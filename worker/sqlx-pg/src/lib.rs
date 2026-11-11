//! 給 Workers 版用的最小 SQLx 相容層：API 形狀與後端用到的 SQLx 子集相同，底層是經 Hyperdrive 連到
//! PostgreSQL（試行用 Supabase）的 tokio-postgres。
//!
//! 只實作後端實際用到的部分：`query`／`query_as`／`query_scalar`、`bind`、`fetch_*`、`execute`、
//! `PgPool`／`PgConnection`／`Transaction`、`FromRow`（借用 serde 的 derive）、`types::Json` 與錯誤分類。
//!
//! 連線：Workers 不能跨請求共用 socket，所以連線池只存在一個請求之內（Hyperdrive 在另一端做真正的連線池）。
//! - [`scope`]（每個請求、排程各包一層）是一個小連線池：查詢或交易借一條閒置連線，沒有才連新的，
//!   用完放回。每條新連線都要做一次啟動與 SCRAM 驗證（wasm 裡約數 ms CPU），所以盡量重用。
//! - 交易獨佔借來的連線直到 `commit`／`rollback`（與 SQLx 相同）；沒有提交就被丟掉的交易送出
//!   `ROLLBACK` 後關閉該連線，不放回池裡。
//! - 背景工作用 [`inherit`] 沿用請求的連線池（`waitUntil` 仍屬於同一個請求）。
//! - 不在任何 scope 裡時，每次查詢臨時開一條連線。
//!
//! 每個查詢都用 `query_typed`／`execute_typed`：參數帶著型別一起送，一次往返，不需要 prepare
//! （Hyperdrive 的交易模式連線池下 prepared statement 不能跨交易使用）。

mod arg;
mod de;
mod error;

use std::{
    future::Future,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use serde::de::DeserializeOwned;
use tokio_postgres::{Client, Config, NoTls, Row, config::SslMode};
use worker::{SecureTransport, Socket, postgres_tls::PassthroughTls, send::SendFuture};

pub use arg::{Arg, Encode};
pub use error::{DatabaseError, Error, PgDatabaseError};
/// 以 serde 的 `Deserialize` 當作 `FromRow`：欄位依名稱對應，`(A, B)` tuple 依欄位順序對應。
pub use serde::Deserialize as FromRow;

pub mod types {
    use serde::{Deserialize, Deserializer, Serialize};
    use tokio_postgres::types::Type;

    /// `jsonb` 欄位。
    #[derive(Debug, Clone)]
    pub struct Json<T>(pub T);

    impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Json<T> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            T::deserialize(d).map(Json)
        }
    }

    impl<T: Serialize> crate::Encode for Json<T> {
        fn encode(&self) -> crate::Arg {
            crate::Arg::Json(serde_json::to_value(&self.0).unwrap_or(serde_json::Value::Null))
        }
        fn null_type() -> Type {
            Type::JSONB
        }
    }
}

pub mod postgres {
    pub use crate::PgPool;
}

/// 只為了讓 `Executor<Database = Postgres>` 這類簽章照舊可用。
pub struct Postgres;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Hyperdrive 給的連線資訊（字串，可以跨執行緒標記傳遞；真正的 socket 在查詢時才開）。
#[derive(Clone)]
pub struct Pool {
    info: Arc<ConnInfo>,
}

struct ConnInfo {
    host: String,
    port: u16,
    config: Config,
}

pub type PgPool = Pool;

/// 一個範圍（請求）內的閒置連線。
#[derive(Default)]
struct Idle(std::sync::Mutex<Vec<Arc<Client>>>);

impl Idle {
    fn take(&self) -> Option<Arc<Client>> {
        let mut idle = self.0.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(c) = idle.pop() {
            if !c.is_closed() {
                return Some(c);
            }
        }
        None
    }

    fn put(&self, client: Arc<Client>) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(client);
    }
}

tokio::task_local! {
    static IDLE: Arc<Idle>;
}

/// 在 `fut` 執行期間共用一個連線池（第一次查詢時才連）。池裡的連線在最後一個使用者結束後關閉。
pub async fn scope<F: Future>(fut: F) -> F::Output {
    IDLE.scope(Arc::default(), fut).await
}

/// 沿用目前範圍的連線池執行 `fut`（給背景工作用）；不在範圍裡時開新的。
pub fn inherit<F: Future>(fut: F) -> impl Future<Output = F::Output> {
    let idle = IDLE.try_with(Arc::clone).unwrap_or_default();
    IDLE.scope(idle, fut)
}

/// 借出的一條連線；丟掉時放回借出的連線池（`reusable` 為假或連線已斷時不放回）。
struct Lease {
    client: Arc<Client>,
    home: Option<Arc<Idle>>,
    reusable: bool,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.reusable
            && !self.client.is_closed()
            && let Some(home) = &self.home
        {
            home.put(self.client.clone());
        }
    }
}

impl Pool {
    pub fn new(hyperdrive: &worker::Hyperdrive) -> Result<Self> {
        let config: Config = hyperdrive
            .connection_string()
            .parse()
            .map_err(|e: tokio_postgres::Error| Error::Driver(e.to_string()))?;
        Ok(Self {
            info: Arc::new(ConnInfo {
                host: hyperdrive.host(),
                port: hyperdrive.port(),
                config,
            }),
        })
    }

    /// 開一條新連線。
    fn connect(&self) -> impl Future<Output = Result<Arc<Client>>> + Send {
        let info = self.info.clone();
        SendFuture::new(async move {
            // 本機開發（`localConnectionString` 指向沒有 TLS 的資料庫）時可設 sslmode=disable
            let plain = info.config.get_ssl_mode() == SslMode::Disable;
            let socket = Socket::builder()
                .secure_transport(if plain {
                    SecureTransport::Off
                } else {
                    SecureTransport::StartTls
                })
                .connect(info.host.clone(), info.port)
                .map_err(|e| Error::Driver(e.to_string()))?;
            let client = if plain {
                let (client, conn) = info
                    .config
                    .connect_raw(socket, NoTls)
                    .await
                    .map_err(Error::from_pg)?;
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = conn.await;
                });
                client
            } else {
                let (client, conn) = info
                    .config
                    .connect_raw(socket, PassthroughTls)
                    .await
                    .map_err(Error::from_pg)?;
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = conn.await;
                });
                client
            };
            Ok(Arc::new(client))
        })
    }

    /// 從目前範圍的連線池借一條連線；沒有閒置的就連一條新的。
    fn lease(&self) -> impl Future<Output = Result<Lease>> + Send {
        let pool = self.clone();
        let home = IDLE.try_with(Arc::clone).ok();
        async move {
            let client = match home.as_deref().and_then(Idle::take) {
                Some(c) => c,
                None => pool.connect().await?,
            };
            Ok(Lease {
                client,
                home,
                reusable: true,
            })
        }
    }

    /// 開始交易：借一條連線獨佔使用，送出 `BEGIN`。
    pub fn begin(&self) -> impl Future<Output = Result<Transaction<'static, Postgres>>> + Send {
        let lease = self.lease();
        async move {
            let mut lease = lease.await?;
            if let Err(e) = lease.client.batch_execute("BEGIN").await {
                lease.reusable = false;
                return Err(Error::from_pg(e));
            }
            Ok(Transaction {
                conn: PgConnection { lease },
                open: true,
                _p: PhantomData,
            })
        }
    }

    pub fn acquire(&self) -> impl Future<Output = Result<PoolConnection>> + Send {
        let lease = self.lease();
        async move {
            Ok(PoolConnection(PgConnection {
                lease: lease.await?,
            }))
        }
    }

    /// 在同一個交易裡依序執行多個語句（全部成功或全部回滾）；回傳各語句影響的列數。
    pub fn batch(
        &self,
        queries: Vec<Query>,
    ) -> impl Future<Output = Result<Vec<QueryResult>>> + Send {
        let begin = self.begin();
        async move {
            if queries.is_empty() {
                return Ok(Vec::new());
            }
            let mut tx = begin.await?;
            let mut results = Vec::with_capacity(queries.len());
            for q in queries {
                results.push(q.execute(&mut tx).await?);
            }
            tx.commit().await?;
            Ok(results)
        }
    }
}

/// 一條連線。
pub struct PgConnection {
    lease: Lease,
}

pub struct PoolConnection(PgConnection);

impl Deref for PoolConnection {
    type Target = PgConnection;
    fn deref(&self) -> &PgConnection {
        &self.0
    }
}

impl DerefMut for PoolConnection {
    fn deref_mut(&mut self) -> &mut PgConnection {
        &mut self.0
    }
}

/// 交易。沒有 `commit` 就被丟掉時回滾（與 SQLx 相同）。
pub struct Transaction<'c, DB = Postgres> {
    conn: PgConnection,
    open: bool,
    _p: PhantomData<(&'c (), DB)>,
}

impl<DB> Transaction<'_, DB> {
    pub async fn commit(mut self) -> Result<()> {
        self.finish("COMMIT").await
    }

    pub async fn rollback(mut self) -> Result<()> {
        self.finish("ROLLBACK").await
    }

    async fn finish(&mut self, sql: &str) -> Result<()> {
        self.open = false;
        let r = self.conn.lease.client.batch_execute(sql).await;
        // 失敗時連線狀態不明，不放回池裡
        self.conn.lease.reusable = r.is_ok();
        r.map_err(Error::from_pg)
    }
}

impl<DB> Drop for Transaction<'_, DB> {
    fn drop(&mut self) {
        if self.open {
            // 沒有提交：回滾後關閉這條連線（不放回池裡，之後的查詢才不會跑進這個交易）。
            // 連線關閉時 PostgreSQL 也會回滾；明確送一次，Hyperdrive 那端的連線就能立刻回到池裡
            self.conn.lease.reusable = false;
            let client = self.conn.lease.client.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = client.batch_execute("ROLLBACK").await;
            });
        }
    }
}

impl<DB> Deref for Transaction<'_, DB> {
    type Target = PgConnection;
    fn deref(&self) -> &PgConnection {
        &self.conn
    }
}

impl<DB> DerefMut for Transaction<'_, DB> {
    fn deref_mut(&mut self) -> &mut PgConnection {
        &mut self.conn
    }
}

/// 查詢要在哪裡執行。
pub enum Target {
    Pool(Pool),
    Client(Arc<Client>),
}

impl Target {
    /// 執行一個查詢用的連線：連線池借一條（查完歸還），交易或連線直接用。
    async fn lease(self) -> Result<Lease> {
        match self {
            Target::Pool(p) => p.lease().await,
            Target::Client(client) => Ok(Lease {
                client,
                home: None,
                reusable: false,
            }),
        }
    }
}

/// 可以執行查詢的對象：`&PgPool`、`&mut PgConnection`、`&mut Transaction`。
pub trait Executor<'c>: Send + Sized {
    type Database;
    fn into_target(self) -> Target;
}

impl<'c> Executor<'c> for &'c Pool {
    type Database = Postgres;
    fn into_target(self) -> Target {
        Target::Pool(self.clone())
    }
}

impl<'c> Executor<'c> for &'c mut PgConnection {
    type Database = Postgres;
    fn into_target(self) -> Target {
        Target::Client(self.lease.client.clone())
    }
}

impl<'c, 't> Executor<'c> for &'c mut Transaction<'t, Postgres> {
    type Database = Postgres;
    fn into_target(self) -> Target {
        Target::Client(self.conn.lease.client.clone())
    }
}

impl<'c> Executor<'c> for &'c mut PoolConnection {
    type Database = Postgres;
    fn into_target(self) -> Target {
        Target::Client(self.0.lease.client.clone())
    }
}

pub trait PgExecutor<'c>: Executor<'c, Database = Postgres> {}
impl<'c, T: Executor<'c, Database = Postgres>> PgExecutor<'c> for T {}

#[derive(Debug, Clone, Copy)]
pub struct QueryResult {
    rows_affected: u64,
}

impl QueryResult {
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

/// 一個查詢語句與它綁定的參數。
#[derive(Debug, Clone)]
pub struct Query {
    sql: String,
    args: Vec<Arg>,
}

pub fn query(sql: &str) -> Query {
    Query {
        sql: sql.to_string(),
        args: Vec::new(),
    }
}

pub fn query_as<O>(sql: &str) -> QueryAs<O> {
    QueryAs {
        inner: query(sql),
        _o: PhantomData,
    }
}

pub fn query_scalar<O>(sql: &str) -> QueryScalar<O> {
    QueryScalar {
        inner: query(sql),
        _o: PhantomData,
    }
}

/// 查回的資料：欄位名稱與各列的值。
pub(crate) struct Rows {
    pub columns: Arc<[String]>,
    pub rows: Vec<Vec<de::Cell>>,
}

fn convert(rows: Vec<Row>) -> Result<Rows> {
    let columns: Arc<[String]> = match rows.first() {
        Some(r) => r.columns().iter().map(|c| c.name().to_string()).collect(),
        None => Arc::from(Vec::<String>::new()),
    };
    let rows = rows
        .iter()
        .map(|r| (0..r.len()).map(|i| de::Cell::from_row(r, i)).collect())
        .collect::<Result<_>>()?;
    Ok(Rows { columns, rows })
}

impl Query {
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.args.push(value.encode());
        self
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn execute<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<QueryResult>> + Send {
        let target = executor.into_target();
        async move {
            let lease = target.lease().await?;
            let params: Vec<_> = self.args.iter().map(Arg::param).collect();
            let n = lease
                .client
                .execute_typed(&self.sql, &params)
                .await
                .map_err(Error::from_pg)?;
            Ok(QueryResult { rows_affected: n })
        }
    }

    /// 不指定型別地取回所有列（取代 SQLx 的 `try_map(|row: PgRow| …)`）。
    pub fn fetch_all_rows<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Vec<AnyRow>>> + Send {
        let fut = self.fetch_rows(executor.into_target());
        async move {
            let rows = fut.await?;
            Ok(rows
                .rows
                .into_iter()
                .map(|cells| AnyRow {
                    columns: rows.columns.clone(),
                    cells,
                })
                .collect())
        }
    }

    /// 與 SQLx 的 `Query::fetch_all` 相同（回傳不指定型別的列）。
    pub fn fetch_all<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Vec<AnyRow>>> + Send {
        self.fetch_all_rows(executor)
    }

    async fn fetch_rows(self, target: Target) -> Result<Rows> {
        let lease = target.lease().await?;
        let params: Vec<_> = self.args.iter().map(Arg::param).collect();
        let rows = lease
            .client
            .query_typed(&self.sql, &params)
            .await
            .map_err(Error::from_pg)?;
        convert(rows)
    }
}

/// 一列資料：可依欄位名稱取值，或整列轉成某個型別（多出的欄位忽略）。
#[derive(Debug, Clone)]
pub struct AnyRow {
    columns: Arc<[String]>,
    cells: Vec<de::Cell>,
}

impl AnyRow {
    pub fn try_get<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let i = self
            .columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| Error::Decode(format!("no column {name}")))?;
        de::from_scalar(vec![self.cells[i].clone()])
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        de::from_row(&self.columns, self.cells.clone())
    }
}

pub struct QueryAs<O> {
    inner: Query,
    _o: PhantomData<fn() -> O>,
}

impl<O: DeserializeOwned + Send + 'static> QueryAs<O> {
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.inner = self.inner.bind(value);
        self
    }

    pub fn fetch_all<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Vec<O>>> + Send {
        let fut = self.inner.fetch_rows(executor.into_target());
        async move {
            let rows = fut.await?;
            rows.rows
                .into_iter()
                .map(|r| de::from_row(&rows.columns, r))
                .collect()
        }
    }

    pub fn fetch_optional<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Option<O>>> + Send {
        let fut = self.inner.fetch_rows(executor.into_target());
        async move {
            let rows = fut.await?;
            match rows.rows.into_iter().next() {
                Some(r) => de::from_row(&rows.columns, r).map(Some),
                None => Ok(None),
            }
        }
    }

    pub fn fetch_one<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<O>> + Send {
        let fut = self.fetch_optional(executor);
        async move { fut.await?.ok_or(Error::RowNotFound) }
    }

    pub fn into_query(self) -> Query {
        self.inner
    }
}

pub struct QueryScalar<O> {
    inner: Query,
    _o: PhantomData<fn() -> O>,
}

impl<O: DeserializeOwned + Send + 'static> QueryScalar<O> {
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.inner = self.inner.bind(value);
        self
    }

    pub fn fetch_all<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Vec<O>>> + Send {
        let fut = self.inner.fetch_rows(executor.into_target());
        async move {
            let rows = fut.await?;
            rows.rows.into_iter().map(de::from_scalar).collect()
        }
    }

    pub fn fetch_optional<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Option<O>>> + Send {
        let fut = self.inner.fetch_rows(executor.into_target());
        async move {
            let rows = fut.await?;
            match rows.rows.into_iter().next() {
                Some(r) => de::from_scalar(r).map(Some),
                None => Ok(None),
            }
        }
    }

    pub fn fetch_one<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<O>> + Send {
        let fut = self.fetch_optional(executor);
        async move { fut.await?.ok_or(Error::RowNotFound) }
    }

    pub fn into_query(self) -> Query {
        self.inner
    }
}
