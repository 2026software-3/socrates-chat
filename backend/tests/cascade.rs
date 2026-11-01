//! 語音對話實驗（cascade）：內部 LLM 端點與 WebSocket 入口的授權、用量與存檔。
//! 全部使用合成資料與假 AI，不連真的 cascade。

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    cascade::{
        HeardTracker, VoiceSession, allow_client_event, allow_server_event, heard_chars,
        record_heard, voice_session_prompt,
    },
    config::Config,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

const SECRET: &str = "synthetic-llm-secret";
const CHAT: &str = "/internal/cascade/v1/chat/completions";
const REPLY: [&str; 2] = [
    "你為什麼",
    "這樣認為？<<<META>>>{\"question_type\":\"clarify\"}",
];

fn enabled() -> Config {
    Config {
        cascade_realtime_url: Some("ws://127.0.0.1:1/v1/realtime".into()),
        cascade_llm_token: Some(SECRET.into()),
        ..test_config()
    }
}

struct Rig {
    app: Router,
    state: AppState,
    ai: Arc<FakeAi>,
}

fn rig(pool: PgPool, scripts: Vec<Script>, config: Config) -> Rig {
    register_fixture_pool(&pool);
    let ai = FakeAi::with(scripts);
    let state = AppState::new(pool, config, Arc::new(FakeIdentity), ai.clone());
    Rig {
        app: app(state.clone()),
        state,
        ai,
    }
}

async fn owner(pool: &PgPool, conversation: &str) -> Uuid {
    let id: Uuid = conversation.parse().unwrap();
    sqlx::query_scalar("SELECT user_id FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn llm_req(token: Option<&str>, body: Value) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(CHAT)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn turn(system: &str, text: &str) -> Value {
    json!({
        "model": "rust",
        "stream": true,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": text},
        ],
    })
}

async fn text_of(res: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// OpenAI 串流回應裡的文字。
fn deltas(body: &str) -> String {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter_map(|v| {
            v["choices"][0]["delta"]["content"]
                .as_str()
                .map(String::from)
        })
        .collect()
}

#[sqlx::test]
async fn the_llm_endpoint_needs_the_shared_secret(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let a = seed_actors(&r.app).await;
    let body = turn("x", "y");

    let none = send(&r.app, llm_req(None, body.clone())).await;
    assert_eq!(none.status(), StatusCode::UNAUTHORIZED);
    let wrong = send(&r.app, llm_req(Some("nope"), body.clone())).await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    // 登入 cookie 不能取代共用密鑰
    let mut with_cookie = llm_req(None, body);
    with_cookie
        .headers_mut()
        .insert(header::COOKIE, a.student.parse().unwrap());
    let res = send(&r.app, with_cookie).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn the_llm_endpoint_is_absent_until_configured(pool: PgPool) {
    let r = rig(pool, vec![], test_config());
    let res = send(&r.app, llm_req(Some(SECRET), turn("x", "y"))).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn the_startup_probe_gets_a_token_without_touching_ai(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let models = Request::builder()
        .uri("/internal/cascade/v1/models")
        .header(header::AUTHORIZATION, format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&r.app, models).await.status(), StatusCode::OK);

    let res = send(
        &r.app,
        llm_req(
            Some(SECRET),
            json!({"model": "rust", "stream": true, "max_tokens": 1,
                   "messages": [{"role": "user", "content": "hi"}]}),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!deltas(&text_of(res).await).is_empty());
    assert_eq!(r.ai.request_count(), 0);
}

#[sqlx::test]
async fn an_unknown_voice_session_is_refused(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let res = send(
        &r.app,
        llm_req(Some(SECRET), turn(&voice_session_prompt("forged"), "你好")),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(r.ai.request_count(), 0);
}

#[sqlx::test]
async fn a_spoken_turn_runs_the_normal_reply_pipeline(pool: PgPool) {
    let r = rig(pool.clone(), vec![Reply(REPLY.to_vec())], enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let id: Uuid = conv.parse().unwrap();
    let session = r
        .state
        .voice_sessions
        .open(owner(&pool, &conv).await, id)
        .unwrap();

    // cascade 打斷後會在 system 後面加註解，代號要仍然認得
    let system = format!(
        "{} A reply was interrupted.",
        voice_session_prompt(session.token())
    );
    let res = send(
        &r.app,
        llm_req(
            Some(SECRET),
            turn(&system, "我的信箱是 student@example.test"),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = text_of(res).await;
    assert_eq!(deltas(&body), "你為什麼這樣認為？");
    assert!(body.trim_end().ends_with("data: [DONE]"));

    // 走同一條管線：OpenAI 拿到的是遮蔽後的文字與專案的 system prompt，不是 cascade 的
    let request = r.ai.requests.lock().unwrap().last().cloned().unwrap();
    assert!(!request.system.contains("voice-session"));
    assert!(
        !request
            .messages
            .iter()
            .any(|m| m.content.contains("student@example.test"))
    );

    let detail = send(
        &r.app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    let detail = json_body(detail).await;
    let messages = detail["messages"].as_array().unwrap();
    let student = messages.iter().rfind(|m| m["role"] == "student").unwrap();
    assert_eq!(student["source"], "speech-cascade");
    assert_eq!(messages.last().unwrap()["role"], "ai");

    let usage = json_body(send(&r.app, get("/api/me/usage", Some(&a.student))).await).await;
    assert_eq!(usage["used"], 1);
}

/// 依序送出兩輪 cascade 的請求，回傳資料庫裡這兩則學生訊息。
async fn saved_speech(pool: PgPool, first: Value, second: Value) -> Vec<String> {
    let r = rig(
        pool.clone(),
        vec![Reply(REPLY.to_vec()), Reply(REPLY.to_vec())],
        enabled(),
    );
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let id: Uuid = conv.parse().unwrap();
    let session = r
        .state
        .voice_sessions
        .open(owner(&pool, &conv).await, id)
        .unwrap();
    let system = voice_session_prompt(session.token());
    for mut body in [first, second] {
        body["messages"][0]["content"] = json!(system);
        let res = send(&r.app, llm_req(Some(SECRET), body)).await;
        assert_eq!(res.status(), StatusCode::OK);
        text_of(res).await;
    }
    let detail = send(
        &r.app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    json_body(detail).await["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["source"] == "speech-cascade")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect()
}

fn history(messages: &[(&str, &str)]) -> Value {
    let mut all = vec![json!({"role": "system", "content": ""})];
    all.extend(
        messages
            .iter()
            .map(|(role, content)| json!({"role": role, "content": content})),
    );
    json!({"model": "rust", "stream": true, "messages": all})
}

#[sqlx::test]
async fn a_turn_cascade_joined_after_an_unheard_reply_is_saved_once(pool: PgPool) {
    let saved = saved_speech(
        pool,
        history(&[("user", "合成的第一句")]),
        // 回覆一個字都沒被聽到：cascade 把回覆移除，前後兩句以空格併成一則
        history(&[("user", "合成的第一句 合成的第二句")]),
    )
    .await;
    // 回覆沒被聽到，接著說的是同一句話：併回原本那則，不多算一回合
    assert_eq!(saved, ["合成的第一句 合成的第二句"]);
}

#[sqlx::test]
async fn a_student_repeating_themselves_is_not_trimmed(pool: PgPool) {
    let saved = saved_speech(
        pool,
        history(&[("user", "合成的第一句")]),
        history(&[
            ("user", "合成的第一句"),
            ("assistant", "你為什麼這樣認為？"),
            ("user", "合成的第一句 我再說一次"),
        ]),
    )
    .await;
    assert_eq!(saved, ["合成的第一句", "合成的第一句 我再說一次"]);
}

/// 一場開好語音會話的對話。
struct Voice {
    r: Rig,
    a: Actors,
    conv: String,
    id: Uuid,
    session: VoiceSession,
    system: String,
    pool: PgPool,
}

async fn voice(pool: PgPool, scripts: Vec<Script>) -> Voice {
    let r = rig(pool.clone(), scripts, enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let id: Uuid = conv.parse().unwrap();
    let session = r
        .state
        .voice_sessions
        .open(owner(&pool, &conv).await, id)
        .unwrap();
    let system = voice_session_prompt(session.token());
    Voice {
        r,
        a,
        conv,
        id,
        session,
        system,
        pool,
    }
}

impl Voice {
    async fn say(&self, messages: &[(&str, &str)]) -> axum::response::Response {
        let mut body = history(messages);
        body["messages"][0]["content"] = json!(self.system);
        send(&self.r.app, llm_req(Some(SECRET), body)).await
    }

    async fn detail(&self) -> Vec<(String, String)> {
        let path = format!("/api/conversations/{}", self.conv);
        let res = send(&self.r.app, get(&path, Some(&self.a.student))).await;
        json_body(res).await["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap().to_string(),
                    m["content"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    async fn speech(&self) -> Vec<String> {
        let path = format!("/api/conversations/{}", self.conv);
        let res = send(&self.r.app, get(&path, Some(&self.a.student))).await;
        json_body(res).await["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["source"] == "speech-cascade")
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect()
    }
}

#[sqlx::test]
async fn a_failed_reply_tells_the_browser_and_refuses_the_next_turn(pool: PgPool) {
    let mut v = voice(pool, vec![Fail, Fail, Reply(REPLY.to_vec())]).await;
    let mut notices = v.session.take_notices().unwrap();

    let res = v.say(&[("user", "合成的第一句")]).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!deltas(&text_of(res).await).is_empty());
    assert!(
        notices
            .recv()
            .await
            .unwrap()
            .contains("socrates.turn.failed")
    );

    // 上一句還沒有回覆：另一個分頁或用戶端也不能再疊上去
    let res = v
        .say(&[
            ("user", "合成的第一句"),
            ("assistant", "x"),
            ("user", "第二句"),
        ])
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "unanswered_message");
    assert_eq!(v.speech().await, ["合成的第一句"]);
    assert_eq!(v.r.ai.request_count(), 2);
}

#[sqlx::test]
async fn a_reply_failing_midway_is_not_a_finished_turn(pool: PgPool) {
    let mut v = voice(pool, vec![FailAfter(vec!["你為什麼"])]).await;
    let mut notices = v.session.take_notices().unwrap();

    let res = v.say(&[("user", "合成的第一句")]).await;
    assert_eq!(deltas(&text_of(res).await), "你為什麼");
    assert!(
        notices
            .recv()
            .await
            .unwrap()
            .contains("socrates.turn.failed")
    );
    assert_eq!(v.detail().await.last().unwrap().0, "student");
}

#[sqlx::test]
async fn a_finished_turn_sends_no_notice(pool: PgPool) {
    let mut v = voice(pool, vec![Reply(REPLY.to_vec())]).await;
    let mut notices = v.session.take_notices().unwrap();
    text_of(v.say(&[("user", "合成的第一句")]).await).await;
    assert!(notices.try_recv().is_err());
}

#[sqlx::test]
async fn a_refused_turn_stays_part_of_the_next_join(pool: PgPool) {
    let v = voice(
        pool.clone(),
        vec![Reply(REPLY.to_vec()), Reply(REPLY.to_vec())],
    )
    .await;
    text_of(v.say(&[("user", "第一句")]).await).await;

    let set_status = |status: &'static str| {
        sqlx::query("UPDATE conversations SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(v.id)
            .execute(&pool)
    };
    set_status("ended").await.unwrap();
    let res = v
        .say(&[("user", "第一句"), ("assistant", "x"), ("user", "第二句")])
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    set_status("active").await.unwrap();

    // 第二句沒有存到；cascade 把它併進下一句時不能當成已存過
    let res = v
        .say(&[
            ("user", "第一句"),
            ("assistant", "x"),
            ("user", "第二句 第三句"),
        ])
        .await;
    text_of(res).await;
    assert_eq!(v.speech().await, ["第一句", "第二句 第三句"]);
}

#[sqlx::test]
async fn a_late_join_waits_for_the_turn_before_it(pool: PgPool) {
    let pace = std::time::Duration::from_millis(100);
    let v = voice(
        pool,
        vec![
            Paced(vec!["回", "覆", "一"], pace),
            Reply(REPLY.to_vec()),
            Reply(REPLY.to_vec()),
        ],
    )
    .await;
    // 第一輪的回覆還在產生，後兩個請求都得等它
    let first = v.say(&[("user", "第一句")]).await;
    let second = [("user", "第一句"), ("assistant", "x"), ("user", "第二句")];
    let joined = [
        ("user", "第一句"),
        ("assistant", "x"),
        ("user", "第二句 第三句"),
    ];
    let (a, b) = tokio::join!(v.say(&second), async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        v.say(&joined).await
    });
    text_of(first).await;
    text_of(a).await;
    text_of(b).await;
    assert_eq!(v.speech().await, ["第一句", "第二句 第三句"]);
}

#[sqlx::test]
async fn a_partly_heard_reply_keeps_the_turns_apart(pool: PgPool) {
    let v = voice(
        pool,
        vec![Reply(vec!["聽到一部分的回覆"]), Reply(REPLY.to_vec())],
    )
    .await;
    text_of(v.say(&[("user", "第一句")]).await).await;
    record_heard(&v.r.state, v.id, 4).await;

    // cascade 只算整句所以仍會併句，但學生其實聽到一部分，接著說的是回應
    text_of(v.say(&[("user", "第一句 第二句")]).await).await;
    assert_eq!(v.speech().await, ["第一句", "第二句"]);
    assert!(v.detail().await.iter().any(|m| m.1 == "聽到一部"));
}

#[sqlx::test]
async fn the_ai_and_the_api_see_only_what_was_heard(pool: PgPool) {
    let v = voice(
        pool.clone(),
        vec![
            Reply(vec!["你聽到這句。沒聽到這句。"]),
            Reply(REPLY.to_vec()),
        ],
    )
    .await;
    text_of(v.say(&[("user", "第一句")]).await).await;

    record_heard(&v.r.state, v.id, 5).await;

    let ai = |d: Vec<(String, String)>| d.into_iter().rfind(|m| m.0 == "ai").unwrap().1;
    assert_eq!(ai(v.detail().await), "你聽到這句");
    text_of(
        v.say(&[
            ("user", "第一句"),
            ("assistant", "你聽到這句"),
            ("user", "第二句"),
        ])
        .await,
    )
    .await;
    let request = v.r.ai.requests.lock().unwrap().last().cloned().unwrap();
    assert!(request.messages.iter().any(|m| m.content == "你聽到這句"));
    assert!(
        !request
            .messages
            .iter()
            .any(|m| m.content.contains("沒聽到"))
    );

    // 完整回覆仍留在資料庫供稽核
    let full: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messages WHERE content LIKE '%沒聽到%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(full, 1);
}

#[sqlx::test]
async fn a_reply_nobody_heard_leaves_the_history(pool: PgPool) {
    let v = voice(
        pool,
        vec![Reply(vec!["完全沒聽到的回覆"]), Reply(REPLY.to_vec())],
    )
    .await;
    text_of(v.say(&[("user", "第一句")]).await).await;

    record_heard(&v.r.state, v.id, 0).await;

    // cascade 也把回覆移除並併句；這不是未回覆的訊息，要能繼續
    let res = v.say(&[("user", "第一句 第二句")]).await;
    assert_eq!(res.status(), StatusCode::OK);
    text_of(res).await;
    assert_eq!(v.speech().await, ["第一句 第二句"]);
    let request = v.r.ai.requests.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.messages.last().unwrap().content, "第一句 第二句");
    let turns: i32 = sqlx::query_scalar("SELECT turn_count FROM conversations WHERE id = $1")
        .bind(v.id)
        .fetch_one(&v.pool)
        .await
        .unwrap();
    assert_eq!(turns, 1);
    assert!(
        !request
            .messages
            .iter()
            .any(|m| m.content.contains("沒聽到"))
    );
    assert!(!v.detail().await.iter().any(|m| m.1.contains("沒聽到")));
}

#[sqlx::test]
async fn the_summary_sees_only_what_was_heard(pool: PgPool) {
    let v = voice(
        pool,
        vec![
            Reply(vec!["你聽到這句。沒聽到這句。"]),
            Reply(vec!["合成的總結"]),
        ],
    )
    .await;
    text_of(v.say(&[("user", "第一句")]).await).await;
    record_heard(&v.r.state, v.id, 5).await;

    let path = format!("/api/conversations/{}/end", v.conv);
    let res = send(&v.r.app, json_req("POST", &path, Some(&v.a.student), None)).await;
    assert!(res.status().is_success());
    for _ in 0..100 {
        if v.r.ai.request_count() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let request = v.r.ai.requests.lock().unwrap().last().cloned().unwrap();
    let all: String = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert!(all.contains("你聽到這句"));
    assert!(!all.contains("沒聽到"));
}

#[sqlx::test]
async fn a_cut_during_generation_applies_when_the_reply_is_saved(pool: PgPool) {
    let pace = std::time::Duration::from_millis(100);
    let v = voice(
        pool,
        vec![Paced(vec!["你聽到這句。", "沒聽到這句。"], pace)],
    )
    .await;
    let res = v.say(&[("user", "第一句")]).await;

    record_heard(&v.r.state, v.id, 5).await;

    text_of(res).await;
    let ai = v.detail().await.into_iter().rfind(|m| m.0 == "ai").unwrap();
    assert_eq!(ai.1, "你聽到這句");
}

#[sqlx::test]
async fn an_unheard_reply_can_be_generated_again(pool: PgPool) {
    let v = voice(
        pool,
        vec![
            Reply(vec!["完全沒聽到的回覆"]),
            Reply(vec!["重新產生的回覆"]),
        ],
    )
    .await;
    text_of(v.say(&[("user", "第一句")]).await).await;
    record_heard(&v.r.state, v.id, 0).await;

    let path = format!("/api/conversations/{}/stream", v.conv);
    let res = send(&v.r.app, get(&path, Some(&v.a.student))).await;
    assert_eq!(res.status(), StatusCode::OK);
    text_of(res).await;
    let ai = v.detail().await.into_iter().rfind(|m| m.0 == "ai").unwrap();
    assert_eq!(ai.1, "重新產生的回覆");
}

#[test]
fn cascade_events_say_how_much_was_heard() {
    let heard = json!({"type": "sglang.response.heard", "text_end": 12});
    assert_eq!(heard_chars(&heard), Some(12));
    let cancelled = json!({"type": "response.done", "response": {"status": "cancelled"}});
    assert_eq!(heard_chars(&cancelled), Some(0));
    let finished = json!({"type": "response.done", "response": {"status": "completed"}});
    assert_eq!(heard_chars(&finished), None);
    assert_eq!(
        heard_chars(&json!({"type": "response.output_audio.delta"})),
        None
    );
}

fn progress(text_end: u64, audio_ms: f64) -> Value {
    json!({"type": "sglang.response.progress", "text_end": text_end, "audio_ms": audio_ms})
}

fn truncate(audio_end_ms: f64) -> Value {
    json!({"type": "conversation.item.truncate", "audio_end_ms": audio_end_ms})
}

fn heard(text_end: u64) -> Value {
    json!({"type": "sglang.response.heard", "text_end": text_end})
}

#[test]
fn a_cut_inside_a_sentence_counts_the_part_that_played() {
    let mut t = HeardTracker::default();
    t.from_cascade(&progress(30, 8000.0));
    t.from_client(&truncate(6000.0));
    assert_eq!(t.from_cascade(&heard(0)), Some(22));
}

#[test]
fn a_cut_in_a_later_sentence_starts_from_the_sentence_before() {
    let mut t = HeardTracker::default();
    t.from_cascade(&progress(10, 3000.0));
    t.from_cascade(&progress(30, 7000.0));
    t.from_client(&truncate(5000.0));
    assert_eq!(t.from_cascade(&heard(10)), Some(20));
}

#[test]
fn a_cut_past_the_known_audio_estimates_from_the_speaking_rate() {
    let mut t = HeardTracker::default();
    t.from_client(&truncate(2000.0));
    assert_eq!(t.from_cascade(&heard(0)), Some(7));

    let mut t = HeardTracker::default();
    t.from_cascade(&progress(10, 3000.0));
    t.from_client(&truncate(4000.0));
    assert_eq!(t.from_cascade(&heard(10)), Some(13));
}

#[test]
fn a_blip_of_audio_does_not_count_as_heard() {
    let mut t = HeardTracker::default();
    t.from_client(&truncate(500.0));
    assert_eq!(t.from_cascade(&heard(0)), Some(0));
}

#[test]
fn a_new_reply_starts_a_new_timeline() {
    let mut t = HeardTracker::default();
    t.from_cascade(&progress(30, 8000.0));
    t.from_cascade(&json!({"type": "response.created"}));
    t.from_client(&truncate(2000.0));
    assert_eq!(t.from_cascade(&heard(0)), Some(7));
}

#[test]
fn the_tracker_never_goes_below_what_cascade_reports() {
    let mut t = HeardTracker::default();
    t.from_cascade(&progress(30, 8000.0));
    t.from_client(&truncate(1000.0));
    assert_eq!(t.from_cascade(&heard(12)), Some(12));
}

#[sqlx::test]
async fn a_user_has_one_voice_session_and_closing_frees_it(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let user = Uuid::new_v4();
    let first = r.state.voice_sessions.open(user, Uuid::new_v4()).unwrap();
    assert!(r.state.voice_sessions.open(user, Uuid::new_v4()).is_none());
    let token = first.token().to_string();
    drop(first);
    assert!(r.state.voice_sessions.lookup(&token).is_none());
    assert!(r.state.voice_sessions.open(user, Uuid::new_v4()).is_some());
}

#[sqlx::test]
async fn the_voice_socket_checks_login_and_ownership(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let other = add_student(&r.app, &a, "other-student", "other@example.test").await;
    let path = format!("/api/conversations/{conv}/voice");

    assert_eq!(
        send(&r.app, get(&path, None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r.app, get(&path, Some(&other))).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&r.app, get(&path, Some(&a.teacher))).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
async fn the_voice_socket_refuses_other_sites(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let req = Request::builder()
        .uri(format!("/api/conversations/{conv}/voice"))
        .header(header::COOKIE, a.student.as_str())
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::empty())
        .unwrap();
    let res = send(&r.app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "csrf");
}

#[sqlx::test]
async fn the_voice_socket_is_unavailable_until_configured(pool: PgPool) {
    let r = rig(pool, vec![], test_config());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let res = send(
        &r.app,
        get(
            &format!("/api/conversations/{conv}/voice"),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

async fn voice_realtime_flag(pool: PgPool, config: Config) -> Value {
    let r = rig(pool, vec![], config);
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let res = send(
        &r.app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    json_body(res).await["voice_realtime"].clone()
}

#[sqlx::test]
async fn the_conversation_says_when_server_voice_is_on(pool: PgPool) {
    assert_eq!(voice_realtime_flag(pool, enabled()).await, true);
}

#[sqlx::test]
async fn the_conversation_says_when_server_voice_is_off(pool: PgPool) {
    assert_eq!(voice_realtime_flag(pool, test_config()).await, false);
}

#[test]
fn the_browser_cannot_change_the_session_or_reach_other_controls() {
    assert!(allow_client_event("input_audio_buffer.append"));
    assert!(allow_client_event("response.cancel"));
    assert!(allow_client_event("conversation.item.truncate"));
    // session.update 能改掉 instructions，也就是別人的會話代號
    assert!(!allow_client_event("session.update"));
    assert!(!allow_client_event("input_audio_buffer.commit"));
    assert!(!allow_client_event("anything.else"));
}

#[test]
fn the_session_token_never_reaches_the_browser() {
    assert!(!allow_server_event("session.created"));
    assert!(!allow_server_event("session.updated"));
    assert!(allow_server_event("session.closed"));
    assert!(allow_server_event("response.output_audio.delta"));
}
