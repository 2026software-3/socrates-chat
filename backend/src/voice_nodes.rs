//! 語音節點登錄（S-19）：伺服器端語音（cascade）可以有多個節點，管理者不必重啟後端就能新增、
//! 暫停或移除。這裡放節點清單的記憶體快取、健康探測與選節點的規則；場次數在 `cascade::VoiceSessions`，
//! 因為「選節點」與「場次加一」必須在同一把鎖內完成。
//!
//! 環境變數 `CASCADE_REALTIME_URL`／`CASCADE_LLM_TOKEN` 是固定存在的「環境節點」，不在資料庫。
//! 健康狀態與場次數都不存資料庫：重啟後每個節點先視為健康，直到探測失敗才降級。

use std::{
    collections::HashMap,
    fmt,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use futures_util::future::join_all;
use reqwest::Url;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{AppState, mutex::lock};

/// 管理 API 的列表以這個固定字串表示環境節點；它也是保留的節點名稱。
pub const ENV_NODE: &str = "env";
/// 每次連線嘗試的逾時。瀏覽器 10 秒沒回應就放棄，兩次嘗試加 `session.update` 要放得進去（暫定）。
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// 背景探測的間隔與單次逾時（暫定）。
pub const PROBE_INTERVAL: Duration = Duration::from_secs(10);
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// 連續失敗這麼多次才標為不健康（暫定）；一次成功即恢復。
pub const FAILURES_TO_DEGRADE: u32 = 3;
/// 節點清單的快取時間，也是 `disabled`／刪除之後進行中的場次最慢多久結束。
pub const LIST_TTL: Duration = Duration::from_secs(2);

/// 節點的識別：環境節點，或登錄表的一列。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeId {
    Env,
    Db(Uuid),
}

impl fmt::Display for NodeId {
    /// 日誌只用這個識別，不含網址或名稱以外的內容。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env => f.write_str(ENV_NODE),
            Self::Db(id) => write!(f, "{id}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeStatus {
    Active,
    Draining,
    Disabled,
}

impl NodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "active" => Some(Self::Active),
            "draining" => Some(Self::Draining),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub realtime_url: String,
    /// 環境節點預設不設上限
    pub max_sessions: Option<usize>,
    pub status: NodeStatus,
}

impl Node {
    /// 環境節點：網址與密鑰都設了才存在。
    pub fn env(config: &crate::config::Config) -> Option<Self> {
        config.cascade_voice_url().map(|url| Self {
            id: NodeId::Env,
            name: ENV_NODE.into(),
            realtime_url: url.to_string(),
            max_sessions: config.cascade_max_sessions,
            status: NodeStatus::Active,
        })
    }

    /// 新場次的排序：場次最少者優先，相同時環境節點優先，其次依名稱。
    pub fn pick_key(&self, sessions: usize) -> (usize, u8, &str) {
        (
            sessions,
            u8::from(self.id != NodeId::Env),
            self.name.as_str(),
        )
    }
}

type Row = (Uuid, String, String, i32, String);

/// 由即時端點網址推得健康端點：`ws→http`、`wss→https`，路徑換成 `/health`。
pub fn health_url(realtime_url: &str) -> Option<Url> {
    let mut url = Url::parse(realtime_url).ok()?;
    let scheme = match url.scheme() {
        "ws" => "http",
        "wss" => "https",
        _ => return None,
    };
    url.set_scheme(scheme).ok()?;
    url.set_path("/health");
    url.set_query(None);
    url.set_fragment(None);
    // reqwest 會把網址裡的帳密當成 Basic auth 送出；探測不帶任何憑證
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    Some(url)
}

#[derive(Default)]
struct Cache {
    nodes: Vec<Node>,
    checked: Option<Instant>,
    /// 每次 `invalidate()` 加一：讀取開始前記下版本，回來時版本變了就丟棄這次讀到的舊值。
    version: u64,
}

/// 節點清單的快取與各節點的健康狀態。
#[derive(Default)]
pub struct VoiceNodes {
    cache: Mutex<Cache>,
    /// 連續失敗次數；沒有紀錄代表健康（啟動時先視為健康）
    failures: Mutex<HashMap<NodeId, u32>>,
}

impl VoiceNodes {
    /// 登錄表的節點（含 `draining`、`disabled`），2 秒 TTL；資料庫讀不到時沿用上一次的值。
    pub async fn registered(&self, pool: &PgPool) -> Vec<Node> {
        let version = {
            let mut cache = lock(&self.cache);
            if cache.checked.is_some_and(|at| at.elapsed() < LIST_TTL) {
                return cache.nodes.clone();
            }
            // 先記下檢查時間：資料庫讀不到時也只每個 TTL 重試一次
            cache.checked = Some(Instant::now());
            cache.version
        };
        let read = tokio::time::timeout(
            Duration::from_secs(1),
            sqlx::query_as::<_, Row>(
                "SELECT id, name, realtime_url, max_sessions, status FROM voice_nodes ORDER BY name",
            )
            .fetch_all(pool),
        )
        .await;
        let nodes = match read {
            Ok(Ok(rows)) => Some(
                rows.into_iter()
                    .filter_map(|(id, name, realtime_url, max, status)| {
                        Some(Node {
                            id: NodeId::Db(id),
                            name,
                            realtime_url,
                            max_sessions: usize::try_from(max).ok(),
                            status: NodeStatus::parse(&status)?,
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
            Ok(Err(e)) => {
                tracing::warn!(class = %crate::error::db_error_class(&e), "failed to read voice nodes");
                None
            }
            Err(_) => {
                tracing::warn!("timed out reading voice nodes");
                None
            }
        };
        let mut cache = lock(&self.cache);
        if let Some(nodes) = nodes
            && cache.version == version
        {
            cache.nodes = nodes;
        }
        cache.nodes.clone()
    }

    /// 管理 API 改了節點後立即讓快取失效，這個程序馬上看到新的清單。
    pub fn invalidate(&self) {
        let mut cache = lock(&self.cache);
        cache.checked = None;
        cache.version += 1;
    }

    /// 環境節點加登錄表的所有節點（含非 `active`）。
    pub async fn all(&self, state: &AppState) -> Vec<Node> {
        let mut nodes: Vec<Node> = Node::env(&state.config).into_iter().collect();
        nodes.extend(self.registered(&state.pool).await);
        nodes
    }

    pub fn is_healthy(&self, id: NodeId) -> bool {
        lock(&self.failures)
            .get(&id)
            .is_none_or(|n| *n < FAILURES_TO_DEGRADE)
    }

    /// 連線失敗時立即標為不健康，不必等連續 3 次探測。
    pub fn mark_unhealthy(&self, id: NodeId) {
        lock(&self.failures).insert(id, FAILURES_TO_DEGRADE);
    }

    pub fn record_probe(&self, id: NodeId, ok: bool) {
        let mut failures = lock(&self.failures);
        if ok {
            failures.remove(&id);
        } else {
            let n = failures.entry(id).or_insert(0);
            *n = (*n + 1).min(FAILURES_TO_DEGRADE);
        }
    }

    /// 網址改了或節點刪了：舊的探測結果不再適用。
    pub fn forget(&self, id: NodeId) {
        lock(&self.failures).remove(&id);
    }

    /// 可以接新場次的節點：`active` 且健康，排除指定者。是否已滿由 `VoiceSessions` 在鎖內判斷。
    pub async fn candidates(&self, state: &AppState, exclude: &[NodeId]) -> Vec<Node> {
        self.all(state)
            .await
            .into_iter()
            .filter(|n| {
                n.status == NodeStatus::Active && !exclude.contains(&n.id) && self.is_healthy(n.id)
            })
            .collect()
    }

    /// 存在至少一個 `active` 且健康的節點，不看是否已滿（對話詳情的 `voice_realtime`）。
    pub async fn any_available(&self, state: &AppState) -> bool {
        !self.candidates(state, &[]).await.is_empty()
    }

    /// 進行中的場次還能不能繼續：節點被停用或刪除就不行；`draining` 繼續到結束。
    pub async fn still_serving(&self, state: &AppState, id: NodeId) -> bool {
        self.all(state)
            .await
            .iter()
            .any(|n| n.id == id && n.status != NodeStatus::Disabled)
    }

    /// 探測一次所有 `active` 與 `draining` 節點。
    pub async fn probe_all(&self, state: &AppState) {
        let nodes: Vec<_> = self
            .all(state)
            .await
            .into_iter()
            .filter(|n| n.status != NodeStatus::Disabled)
            .collect();
        if nodes.is_empty() {
            return;
        }
        crate::telemetry::in_component_span("background_job", "voice_nodes_probe", async {
            let job_metric = state.metrics.track_background_job("voice_nodes_probe");
            let results = join_all(nodes.into_iter().map(|node| async move {
                let target = node.id.to_string();
                let ok = crate::telemetry::in_component_span_with_target(
                    "voice_node_probe",
                    "health_check",
                    Some(&target),
                    probe(&node.realtime_url),
                )
                .await;
                if !ok {
                    // 只記節點識別與原因類別，不記網址
                    tracing::warn!(node = %node.id, "voice node health probe failed");
                }
                self.record_probe(node.id, ok);
                ok
            }))
            .await;
            let all_ok = results.into_iter().all(std::convert::identity);
            job_metric.finish(if all_ok { "ok" } else { "degraded" });
        })
        .await;
    }
}

static PROBE_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        // 3xx 不算健康，也不要被導去別處
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
});

/// 健康端點回 2xx 才算健康。
async fn probe(realtime_url: &str) -> bool {
    let Some(url) = health_url(realtime_url) else {
        return false;
    };
    let mut req = PROBE_CLIENT.get(url);
    if let Some(tp) = crate::telemetry::current_traceparent() {
        req = req.header(crate::telemetry::TRACEPARENT_HEADER, tp);
    }
    req.send().await.is_ok_and(|res| res.status().is_success())
}

/// 背景探測：每 10 秒一輪。
pub fn spawn(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROBE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            state.voice_nodes.probe_all(&state).await;
        }
    });
}
