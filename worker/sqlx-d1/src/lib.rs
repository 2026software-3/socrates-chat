//! 給 Workers 版用的最小 SQLx 相容層：API 形狀與後端用到的 SQLx 子集相同，底層改成 Cloudflare D1。
//!
//! 只實作後端實際用到的部分：`query`／`query_as`／`query_scalar`、`bind`、`fetch_*`、`execute`、
//! `PgPool`／`PgConnection`／`Transaction`、`FromRow`（借用 serde 的 derive）、`types::Json` 與錯誤分類。
//!
//! 與 PostgreSQL 版的重要差異：
//! - **交易不具原子性**。D1 不支援互動式交易（`BEGIN`），`begin()`／`commit()` 只是讓既有程式碼能編譯，
//!   語句會逐一立即執行；需要原子性的地方要改用 [`Pool::batch`]。
//! - 參數 `$N` 會改寫成 SQLite 的 `?N`；`now()` 會改寫成 ISO 8601 毫秒字串（與 Rust 端綁定的時間格式一致，
//!   可以直接用字串比較大小）。其餘 PostgreSQL 語法要在呼叫端改成 SQLite。
//! - 布林值存成 0／1，讀取時自動轉回 `bool`；`&[u8]` 存成小寫十六進位字串；陣列存成 JSON 字串。

mod arg;
mod de;
mod error;

use std::{
    future::Future,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use js_sys::{Array, Function, Object, Promise, Reflect};
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures_shim::JsFuture;
use worker::{D1Database, send::SendFuture};

pub use arg::{Arg, Encode};
pub use error::{DatabaseError, Error, register_constraint_aliases};
/// 以 serde 的 `Deserialize` 當作 `FromRow`：欄位依名稱對應，`(A, B)` tuple 依欄位順序對應。
pub use serde::Deserialize as FromRow;

mod wasm_bindgen_futures_shim {
    pub use wasm_bindgen_futures::JsFuture;
}

pub mod types {
    use serde::{Deserialize, Deserializer, Serialize};

    /// 以 JSON 文字存放的欄位（PostgreSQL 的 `jsonb`）。
    #[derive(Debug, Clone)]
    pub struct Json<T>(pub T);

    impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Json<T> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let raw = String::deserialize(d)?;
            serde_json::from_str(&raw)
                .map(Json)
                .map_err(serde::de::Error::custom)
        }
    }

    impl<T: Serialize> crate::Encode for Json<T> {
        fn encode(&self) -> crate::Arg {
            crate::Arg::Text(serde_json::to_string(&self.0).unwrap_or_else(|_| "null".into()))
        }
    }
}

pub mod postgres {
    pub use crate::PgPool;
}

/// 只為了讓 `Executor<Database = Postgres>` 這類簽章照舊可用。
pub struct Postgres;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// 一個 D1 資料庫連線（D1 binding 本身就可以共用，沒有連線池）。
#[derive(Clone)]
pub struct Pool {
    db: Arc<D1Database>,
}

pub type PgPool = Pool;

impl Pool {
    pub fn new(db: D1Database) -> Self {
        Self { db: Arc::new(db) }
    }

    pub fn d1(&self) -> &D1Database {
        &self.db
    }

    /// 開始「交易」。D1 沒有互動式交易：語句逐一立即執行，`commit` 與 `rollback` 不做事。
    pub fn begin(&self) -> impl Future<Output = Result<Transaction<'static, Postgres>>> + Send {
        let conn = PgConnection { pool: self.clone() };
        async move {
            Ok(Transaction {
                conn,
                _p: PhantomData,
            })
        }
    }

    pub fn acquire(&self) -> impl Future<Output = Result<PoolConnection>> + Send {
        let conn = PgConnection { pool: self.clone() };
        async move { Ok(PoolConnection(conn)) }
    }

    /// 以單一 D1 batch 原子地執行多個語句（全部成功或全部回滾）；回傳各語句影響的列數。
    pub fn batch(
        &self,
        queries: Vec<Query>,
    ) -> impl Future<Output = Result<Vec<QueryResult>>> + Send {
        let pool = self.clone();
        SendFuture::new(async move {
            if queries.is_empty() {
                return Ok(Vec::new());
            }
            let mut stmts = Vec::with_capacity(queries.len());
            for q in &queries {
                stmts.push(q.statement(&pool)?);
            }
            let results = pool.db.batch(stmts).await.map_err(Error::from_worker)?;
            Ok(results
                .iter()
                .map(|r| QueryResult {
                    rows_affected: r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0)
                        as u64,
                })
                .collect())
        })
    }

    /// 執行多個以分號分隔的語句（建立 schema、測試資料用）。
    pub fn exec_script(&self, sql: &str) -> impl Future<Output = Result<()>> + Send {
        let pool = self.clone();
        let sql = sql.to_string();
        SendFuture::new(async move {
            pool.db.exec(&sql).await.map_err(Error::from_worker)?;
            Ok(())
        })
    }
}

pub struct PgConnection {
    pool: Pool,
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

pub struct Transaction<'c, DB = Postgres> {
    conn: PgConnection,
    _p: PhantomData<&'c DB>,
}

impl<DB> Transaction<'_, DB> {
    pub async fn commit(self) -> Result<()> {
        Ok(())
    }

    pub async fn rollback(self) -> Result<()> {
        Ok(())
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

/// 可以執行查詢的對象：`&PgPool`、`&mut PgConnection`、`&mut Transaction`。
pub trait Executor<'c>: Send + Sized {
    type Database;
    fn into_pool(self) -> Pool;
}

impl<'c> Executor<'c> for &'c Pool {
    type Database = Postgres;
    fn into_pool(self) -> Pool {
        self.clone()
    }
}

impl<'c> Executor<'c> for &'c mut PgConnection {
    type Database = Postgres;
    fn into_pool(self) -> Pool {
        self.pool.clone()
    }
}

impl<'c, 't> Executor<'c> for &'c mut Transaction<'t, Postgres> {
    type Database = Postgres;
    fn into_pool(self) -> Pool {
        self.conn.pool.clone()
    }
}

impl<'c> Executor<'c> for &'c mut PoolConnection {
    type Database = Postgres;
    fn into_pool(self) -> Pool {
        self.0.pool.clone()
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
        sql: translate(sql),
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

impl Query {
    pub fn bind<T: Encode>(mut self, value: T) -> Self {
        self.args.push(value.encode());
        self
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    fn statement(&self, pool: &Pool) -> Result<worker::D1PreparedStatement> {
        let stmt = pool.db.prepare(self.sql.as_str());
        if self.args.is_empty() {
            return Ok(stmt);
        }
        let values: Vec<JsValue> = self.args.iter().map(Arg::to_js).collect();
        stmt.bind(&values).map_err(Error::from_worker)
    }

    pub fn execute<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<QueryResult>> + Send {
        let pool = executor.into_pool();
        SendFuture::new(async move {
            let stmt = self.statement(&pool)?;
            let result = stmt.run().await.map_err(Error::from_worker)?;
            let changes = result
                .meta()
                .ok()
                .flatten()
                .and_then(|m| m.changes)
                .unwrap_or(0);
            Ok(QueryResult {
                rows_affected: changes as u64,
            })
        })
    }

    /// 不指定型別地取回所有列（取代 SQLx 的 `try_map(|row: PgRow| …)`）。
    pub fn fetch_all_rows<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Vec<AnyRow>>> + Send {
        let fut = self.fetch_rows(executor.into_pool());
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

    fn fetch_rows(self, pool: Pool) -> impl Future<Output = Result<Rows>> + Send {
        SendFuture::new(async move {
            let stmt = self.statement(&pool)?;
            raw_with_columns(&stmt).await
        })
    }
}

/// 呼叫 JS 的 `stmt.raw({ columnNames: true })`：第一列是欄位名稱，其餘是依欄位順序的值。
async fn raw_with_columns(stmt: &worker::D1PreparedStatement) -> Result<Rows> {
    let inner: &JsValue = stmt.inner().as_ref();
    let raw: Function = Reflect::get(inner, &JsValue::from_str("raw"))
        .map_err(Error::from_js)?
        .dyn_into()
        .map_err(Error::from_js)?;
    let options = Object::new();
    Reflect::set(&options, &"columnNames".into(), &JsValue::TRUE).map_err(Error::from_js)?;
    let promise: Promise = raw
        .call1(inner, &options)
        .map_err(Error::from_js)?
        .dyn_into()
        .map_err(Error::from_js)?;
    let value = JsFuture::from(promise).await.map_err(Error::from_js)?;
    let array: Array = value.dyn_into().map_err(Error::from_js)?;
    let mut iter = array.iter();
    let columns: Arc<[String]> = match iter.next() {
        Some(first) => Array::from(&first)
            .iter()
            .map(|c| c.as_string().unwrap_or_default())
            .collect(),
        None => Arc::from(Vec::<String>::new()),
    };
    let rows = iter
        .map(|row| Array::from(&row).iter().map(de::Cell::from_js).collect())
        .collect();
    Ok(Rows { columns, rows })
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
        let fut = self.inner.fetch_rows(executor.into_pool());
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
        let fut = self.inner.fetch_rows(executor.into_pool());
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
        let fut = self.inner.fetch_rows(executor.into_pool());
        async move {
            let rows = fut.await?;
            rows.rows.into_iter().map(de::from_scalar).collect()
        }
    }

    pub fn fetch_optional<'e, E: Executor<'e>>(
        self,
        executor: E,
    ) -> impl Future<Output = Result<Option<O>>> + Send {
        let fut = self.inner.fetch_rows(executor.into_pool());
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

/// D1 寫入的時間格式：UTC、毫秒、`Z` 結尾。Rust 端綁定的時間也用同一格式（見 `arg`），字串比較即時間比較。
pub const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

/// 把 PostgreSQL 風格的 `$N` 參數改成 SQLite 的 `?N`，`now()` 改成 [`NOW_SQL`]；字串常值內不改。
pub fn translate(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 16);
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            out.push(c as char);
            if c == b'\'' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => {
                in_str = true;
                out.push('\'');
                i += 1;
            }
            b'$' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() => {
                out.push('?');
                i += 1;
            }
            b'n' if sql[i..].starts_with("now()")
                && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) =>
            {
                out.push_str(NOW_SQL);
                i += 5;
            }
            _ => {
                // 多位元組 UTF-8 字元原樣複製
                let ch = sql[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::translate;

    #[test]
    fn rewrites_params_and_now_outside_literals() {
        assert_eq!(
            translate("SELECT '$1', now() FROM t WHERE a = $1 AND b = $12 AND c <> 'now()'"),
            format!(
                "SELECT '$1', {} FROM t WHERE a = ?1 AND b = ?12 AND c <> 'now()'",
                super::NOW_SQL
            )
        );
        assert_eq!(translate("SELECT know()"), "SELECT know()");
        assert_eq!(translate("名稱 = $1"), "名稱 = ?1");
    }
}
