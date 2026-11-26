//! F-23.3 一般 API 負載測試（S-10.2、S-10.3）：60 位學生同時上課對話，一般 API（不含 AI）p95 ≤ 500 毫秒。
//!
//! 照 S-10.2「一般 API 的負載測試用假的 AI」：後端是真的（路由、權限、PostgreSQL 都是正式程式碼，
//! 用真實的 TCP 連線打），只有 AI 換成立即回覆的假實作，排除供應商延遲。合成資料，不碰任何部署環境。
//!
//! 負載測試本身標成 `#[ignore]`，CI 不會跑；要量測時用 release 建置手動執行：
//!
//! ```bash
//! cargo test --release --test load_test -- --ignored --nocapture
//! ```
//!
//! 可調整：`LOAD_USERS`（同時學生數，預設 60）、`LOAD_TURNS`（每人送幾則訊息，預設 3）、
//! `LOAD_AI_DELAY_MS`（假 AI 每個片段前等多久，預設 0）。

mod common;

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use common::{APP_URL, FakeIdentity, import_roster, published_activity_with_topic, seed_actors};
use futures_util::{StreamExt, stream};
use socrates_chat_backend::{
    AppState,
    ai::{AiError, AiProvider, AiRequest, AiStream, StructuredCompletion, StructuredRequest},
    app,
};
use sqlx::{Connection, PgConnection, postgres::PgPoolOptions};
use tokio::sync::Barrier;

/// 一般 API 的 p95 門檻（S-10.2）。
const GENERAL_API_P95: Duration = Duration::from_millis(500);

/// 量到的請求種類。`general` 為 false 的不算一般 API（含 AI 回覆時間）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Endpoint {
    Login,
    Me,
    Catalog,
    StartConversation,
    /// 送出訊息到收到第一個 SSE 事件（學生訊息已保存），還沒輪到 AI
    MessageSaved,
    /// 送出訊息到串流結束（含假 AI 的回覆）
    MessageStreamDone,
    ConversationDetail,
    ConversationList,
    EndConversation,
    Summary,
}

impl Endpoint {
    fn label(self) -> &'static str {
        match self {
            Self::Login => "登入（Google callback）",
            Self::Me => "GET /api/me",
            Self::Catalog => "GET /api/catalog",
            Self::StartConversation => "POST /api/conversations",
            Self::MessageSaved => "POST messages：學生訊息已保存",
            Self::MessageStreamDone => "POST messages：串流結束（含假 AI）",
            Self::ConversationDetail => "GET /api/conversations/{id}",
            Self::ConversationList => "GET /api/conversations",
            Self::EndConversation => "POST /api/conversations/{id}/end",
            Self::Summary => "GET /api/conversations/{id}/summary",
        }
    }

    fn general(self) -> bool {
        self != Self::MessageStreamDone
    }
}

/// 第 `p` 百分位數（nearest-rank）：排序後第 ⌈p/100 × n⌉ 個。沒有樣本時為 None。
fn percentile(samples: &[Duration], p: f64) -> Option<Duration> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort();
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

#[derive(Default)]
struct Samples {
    times: BTreeMap<Endpoint, Vec<Duration>>,
    errors: BTreeMap<Endpoint, usize>,
}

impl Samples {
    fn record(&mut self, endpoint: Endpoint, elapsed: Duration) {
        self.times.entry(endpoint).or_default().push(elapsed);
    }

    fn fail(&mut self, endpoint: Endpoint) {
        *self.errors.entry(endpoint).or_default() += 1;
    }

    fn merge(&mut self, other: Samples) {
        for (e, t) in other.times {
            self.times.entry(e).or_default().extend(t);
        }
        for (e, n) in other.errors {
            *self.errors.entry(e).or_default() += n;
        }
    }

    /// 所有一般 API 請求合在一起的 p95（S-10.2 的判定值）。
    fn general_p95(&self) -> Option<Duration> {
        let all: Vec<Duration> = self
            .times
            .iter()
            .filter(|(e, _)| e.general())
            .flat_map(|(_, t)| t.iter().copied())
            .collect();
        percentile(&all, 95.0)
    }

    fn error_count(&self) -> usize {
        self.errors.values().sum()
    }

    /// 每種請求一列：次數、錯誤、p50、p95、最大值（毫秒）。
    fn report(&self) -> String {
        let ms = |d: Option<Duration>| {
            d.map_or("-".to_string(), |d| {
                format!("{:.1}", d.as_secs_f64() * 1000.0)
            })
        };
        let mut out = String::from(
            "| 請求 | 次數 | 錯誤 | p50 (ms) | p95 (ms) | 最大 (ms) |\n| --- | --- | --- | --- | --- | --- |\n",
        );
        let endpoints: std::collections::BTreeSet<Endpoint> = self
            .times
            .keys()
            .chain(self.errors.keys())
            .copied()
            .collect();
        for e in endpoints {
            let t = self.times.get(&e).map(Vec::as_slice).unwrap_or(&[]);
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                e.label(),
                t.len(),
                self.errors.get(&e).copied().unwrap_or(0),
                ms(percentile(t, 50.0)),
                ms(percentile(t, 95.0)),
                ms(t.iter().max().copied()),
            ));
        }
        out
    }
}

/// 立即回覆的假 AI：對話回一段固定的追問，總結回固定的立場；`delay` 模擬每個片段前的產生時間。
struct LoadAi {
    summary_prompt: String,
    delay: Duration,
}

const TUTOR_REPLY: [&str; 3] = [
    "你說的「比較多人」",
    "為什麼比較重要？\n<<<META>>>\n",
    "{\"question_type\":\"clarify\",\"advance\":false}",
];
const SUMMARY_REPLY: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";

#[async_trait]
impl AiProvider for LoadAi {
    async fn complete_json(
        &self,
        _req: StructuredRequest,
    ) -> Result<StructuredCompletion, AiError> {
        // 背景的結構化呼叫（主張分組等）不在本測試量測範圍，給最小的合法 JSON
        Ok(StructuredCompletion {
            content: "{}".into(),
            model: "load-test".into(),
        })
    }

    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        let chunks: Vec<&'static str> = if req.system == self.summary_prompt {
            vec![SUMMARY_REPLY]
        } else {
            TUTOR_REPLY.to_vec()
        };
        let delay = self.delay;
        Ok(stream::iter(chunks)
            .then(move |c| async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                Ok(c.to_string())
            })
            .boxed())
    }
}

fn env_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// 一位學生的一整堂課：登入、看目錄與自己的對話、開始活動對話、來回幾輪、結束、看總結。
async fn one_student(
    base: String,
    n: u64,
    activity: String,
    turns: u64,
    start: Arc<Barrier>,
) -> Samples {
    let mut s = Samples::default();
    // 每位學生各自一個瀏覽器：各自的連線
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    start.wait().await;

    // 登入（S-01.1 Google 流程；測試用的身分提供者）
    let Ok(res) = client
        .get(format!("{base}/api/auth/google/login"))
        .send()
        .await
    else {
        s.fail(Endpoint::Login);
        return s;
    };
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let state = location
        .split("state=")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let t = Instant::now();
    let res = client
        .get(format!(
            "{base}/api/auth/google/callback?code=ok:sub-load{n}:load{n}@example.com&state={state}"
        ))
        .send()
        .await;
    let cookie = res.ok().and_then(|r| {
        r.headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with("sid=") && !v.starts_with("sid=;"))
            .map(|v| v.split(';').next().unwrap().to_string())
    });
    let Some(cookie) = cookie else {
        s.fail(Endpoint::Login);
        return s;
    };
    // flaky-ok: 負載測試本來就是量時間；整支測試是 #[ignore]，CI 不跑，只在手動量測時執行
    s.record(Endpoint::Login, t.elapsed());

    let get = |path: String| {
        client
            .get(format!("{base}{path}"))
            .header(reqwest::header::COOKIE, &cookie)
    };
    let post = |path: String| {
        client
            .post(format!("{base}{path}"))
            .header(reqwest::header::COOKIE, &cookie)
            .header(reqwest::header::ORIGIN, APP_URL)
    };

    timed(&mut s, Endpoint::Me, get("/api/me".into()), 200).await;
    timed(&mut s, Endpoint::Catalog, get("/api/catalog".into()), 200).await;
    timed(
        &mut s,
        Endpoint::ConversationList,
        get("/api/conversations".into()),
        200,
    )
    .await;

    let t = Instant::now();
    let started = post("/api/conversations".into())
        .json(&serde_json::json!({"activity_id": activity}))
        .send()
        .await;
    let conv = match started {
        Ok(r) if r.status().as_u16() == 201 => {
            // flaky-ok: 負載測試本來就是量時間；整支測試是 #[ignore]，CI 不跑，只在手動量測時執行
            s.record(Endpoint::StartConversation, t.elapsed());
            r.json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v["id"].as_str().map(str::to_string))
        }
        _ => None,
    };
    let Some(conv) = conv else {
        s.fail(Endpoint::StartConversation);
        return s;
    };

    for turn in 0..turns {
        let t = Instant::now();
        let res = post(format!("/api/conversations/{conv}/messages"))
            .json(&serde_json::json!({"content": format!("第 {turn} 輪：我會拉桿，因為五個人比一個人多。")}))
            .send()
            .await;
        let Ok(res) = res.and_then(reqwest::Response::error_for_status) else {
            s.fail(Endpoint::MessageSaved);
            continue;
        };
        let mut body = res.bytes_stream();
        let mut text = String::new();
        let mut saved = false;
        while let Some(chunk) = body.next().await {
            let Ok(chunk) = chunk else { break };
            text.push_str(&String::from_utf8_lossy(&chunk));
            // 第一個事件是保存好的學生訊息，之後才輪到 AI
            if !saved && text.contains("event: message") {
                saved = true;
                // flaky-ok: 負載測試本來就是量時間；整支測試是 #[ignore]，CI 不跑，只在手動量測時執行
                s.record(Endpoint::MessageSaved, t.elapsed());
            }
        }
        if !saved {
            s.fail(Endpoint::MessageSaved);
        }
        if text.contains("event: done") {
            // flaky-ok: 負載測試本來就是量時間；整支測試是 #[ignore]，CI 不跑，只在手動量測時執行
            s.record(Endpoint::MessageStreamDone, t.elapsed());
        } else {
            s.fail(Endpoint::MessageStreamDone);
        }
    }

    timed(
        &mut s,
        Endpoint::ConversationDetail,
        get(format!("/api/conversations/{conv}")),
        200,
    )
    .await;
    timed(
        &mut s,
        Endpoint::EndConversation,
        post(format!("/api/conversations/{conv}/end")),
        202,
    )
    .await;
    timed(
        &mut s,
        Endpoint::Summary,
        get(format!("/api/conversations/{conv}/summary")),
        200,
    )
    .await;
    timed(
        &mut s,
        Endpoint::ConversationList,
        get("/api/conversations".into()),
        200,
    )
    .await;
    s
}

/// 送出請求、讀完回應，狀態碼符合才記時間，否則記一次錯誤。
async fn timed(s: &mut Samples, endpoint: Endpoint, req: reqwest::RequestBuilder, expect: u16) {
    let t = Instant::now();
    let ok = match req.send().await {
        Ok(r) => r.status().as_u16() == expect && r.bytes().await.is_ok(),
        Err(_) => false,
    };
    if ok {
        // flaky-ok: 負載測試本來就是量時間；整支測試是 #[ignore]，CI 不跑，只在手動量測時執行
        s.record(endpoint, t.elapsed());
    } else {
        s.fail(endpoint);
    }
}

#[test]
#[ignore = "負載測試：用 --release --ignored 手動執行"]
fn sixty_students_at_once_meet_the_general_api_threshold() {
    let users = env_or("LOAD_USERS", 60);
    let turns = env_or("LOAD_TURNS", 3);
    let delay = Duration::from_millis(env_or("LOAD_AI_DELAY_MS", 0));
    // 正式環境是多執行緒 runtime（#[sqlx::test] 是單執行緒，不適合量負載）
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        // 每次一個全新的資料庫，量完刪掉
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let name = format!("load_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&url).await.unwrap();
        // 名稱只有 load_ 加 UUID 十六進位，沒有外部輸入
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&mut admin).await.unwrap();
        let options: sqlx::postgres::PgConnectOptions = url.parse::<sqlx::postgres::PgConnectOptions>().unwrap().database(&name);
        // 連線池大小與正式環境相同（main.rs：10）
        let pool = PgPoolOptions::new().max_connections(10).connect_with(options).await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();

        let mut config = common::test_config();
        // 逾時用正式環境的值（S-03.5），不用測試縮短過的
        config.ai_first_token_timeout = Duration::from_secs(15);
        config.ai_total_timeout = Duration::from_secs(60);
        config.ai_usage_max = 100_000;
        let ai = Arc::new(LoadAi {
            summary_prompt: socrates_chat_backend::ai::build_summary_prompt("電車難題", &[], "zh-TW"),
            delay,
        });
        common::register_fixture_pool(&pool);
        let router = app(AppState::new(pool.clone(), config, Arc::new(FakeIdentity), ai));

        // 準備：教室、活動、全班的修課名單（不計時）
        let a = seed_actors(&router).await;
        let activity = published_activity_with_topic(&router, &a, "電車難題").await;
        let roster: Vec<String> = (0..users)
            // 測試夾具：依信箱導出合成學號，並替帳號填上信箱（模擬學生自己填好）
            .map(|n| format!("load{n}@example.com"))
            .collect();
        let res = import_roster(&router, &a.admin, &a.classroom, &roster.join("\n")).await;
        assert!(res.status().is_success(), "roster import failed: {}", res.status());

        // 真的開一個 HTTP 服務，學生透過 TCP 連進來
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .unwrap();
        });

        // 全班同時開始（S-10.3）
        let start = Arc::new(Barrier::new(users as usize));
        let began = Instant::now();
        let handles: Vec<_> = (0..users)
            .map(|n| tokio::spawn(one_student(base.clone(), n, activity.clone(), turns, start.clone())))
            .collect();
        let mut all = Samples::default();
        for h in handles {
            all.merge(h.await.unwrap());
        }
        // flaky-ok: 負載測試本來就是量時間；整支測試是 #[ignore]，CI 不跑，只在手動量測時執行
        let wall = began.elapsed();

        println!(
            "\n## 負載測試結果\n\n同時學生 {users} 人、每人 {turns} 輪、假 AI 片段延遲 {} ms、連線池 10、總耗時 {:.1} 秒、建置 {}\n",
            delay.as_millis(),
            wall.as_secs_f64(),
            if cfg!(debug_assertions) { "debug（數字偏慢，請用 --release）" } else { "release" },
        );
        println!("{}", all.report());
        let p95 = all.general_p95().expect("沒有量到任何一般 API 請求");
        println!(
            "一般 API（不含 AI）整體 p95：{:.1} ms（門檻 {} ms），錯誤 {} 次\n",
            p95.as_secs_f64() * 1000.0,
            GENERAL_API_P95.as_millis(),
            all.error_count()
        );

        pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {name} WITH (FORCE)"))).execute(&mut admin).await.unwrap();

        assert_eq!(all.error_count(), 0, "有請求失敗");
        assert!(p95 <= GENERAL_API_P95, "一般 API p95 {p95:?} 超過 {GENERAL_API_P95:?}");
    });
}

#[cfg(test)]
mod stats {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let samples: Vec<Duration> = (1..=100).rev().map(ms).collect();
        assert_eq!(percentile(&samples, 95.0), Some(ms(95)));
        assert_eq!(percentile(&samples, 50.0), Some(ms(50)));
        assert_eq!(percentile(&samples, 100.0), Some(ms(100)));
        assert_eq!(percentile(&[ms(7)], 95.0), Some(ms(7)));
        assert_eq!(percentile(&[], 95.0), None);
    }

    #[test]
    fn p95_of_twenty_samples_is_the_nineteenth() {
        let samples: Vec<Duration> = (1..=20).map(ms).collect();
        assert_eq!(percentile(&samples, 95.0), Some(ms(19)));
    }

    #[test]
    fn general_p95_leaves_out_the_ai_reply() {
        let mut s = Samples::default();
        for n in 1..=20 {
            s.record(Endpoint::Catalog, ms(n));
            // 含 AI 回覆的時間再慢也不算進一般 API
            s.record(Endpoint::MessageStreamDone, ms(10_000));
        }
        assert_eq!(s.general_p95(), Some(ms(19)));
    }

    #[test]
    fn merged_samples_keep_every_time_and_error() {
        let mut a = Samples::default();
        a.record(Endpoint::Me, ms(1));
        a.fail(Endpoint::Login);
        let mut b = Samples::default();
        b.record(Endpoint::Me, ms(2));
        b.fail(Endpoint::Login);
        a.merge(b);
        assert_eq!(a.times[&Endpoint::Me], vec![ms(1), ms(2)]);
        assert_eq!(a.error_count(), 2);
        let report = a.report();
        assert!(report.contains("| GET /api/me | 2 | 0 |"));
        assert!(report.contains("| 登入（Google callback） | 0 | 2 |"));
    }
}
