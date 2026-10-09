//! F-06.2 / F-06.4 / F-07：AI 回覆串流、重試、逾時、階段推進與授權（假 AI、合成資料）。

mod common;

use std::sync::Arc;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

struct Setup {
    app: Router,
    a: Actors,
    ai: Arc<FakeAi>,
    conv: String,
}

async fn setup(pool: PgPool, scripts: Vec<Script>) -> Setup {
    let ai = FakeAi::with(scripts);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": "電車難題", "description": "你會拉桿嗎？"})),
        ),
    )
    .await;
    let act = json_body(res).await["id"].as_str().unwrap().to_string();
    send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"activity_id": act})),
        ),
    )
    .await;
    let conv = json_body(res).await["id"].as_str().unwrap().to_string();
    Setup { app, a, ai, conv }
}

async fn say(s: &Setup, text: &str) {
    let res = send(
        &s.app,
        json_req(
            "POST",
            &format!("/api/conversations/{}/messages", s.conv),
            Some(&s.a.student),
            Some(json!({"content": text})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

async fn stream_events(s: &Setup) -> Vec<(String, Value)> {
    let res = send(
        &s.app,
        get(
            &format!("/api/conversations/{}/stream", s.conv),
            Some(&s.a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    parse_sse(&text_body(res).await)
}

async fn detail(s: &Setup) -> Value {
    json_body(
        send(
            &s.app,
            get(
                &format!("/api/conversations/{}", s.conv),
                Some(&s.a.student),
            ),
        )
        .await,
    )
    .await
}

const REPLY: [&str; 3] = [
    "你說的「正義」",
    "是什麼意思？\n<<<META>>>\n",
    "{\"question_type\":\"clarify\",\"advance\":false}",
];

#[sqlx::test]
async fn streams_reply_saves_it_and_hides_meta(pool: PgPool) {
    let s = setup(pool, vec![Reply(REPLY.to_vec())]).await;
    say(&s, "我認為應該拉桿。").await;
    let events = stream_events(&s).await;

    let deltas: String = events
        .iter()
        .filter(|(n, _)| n == "delta")
        .map(|(_, d)| d["text"].as_str().unwrap())
        .collect();
    assert_eq!(deltas, "你說的「正義」是什麼意思？\n");
    assert!(!deltas.contains("META"));
    let (name, done) = events.last().unwrap();
    assert_eq!(name, "done");
    assert_eq!(done["question_type"], "clarify");
    assert_eq!(done["stage"], 1);
    assert_eq!(done["suggest_end"], false);

    let d = detail(&s).await;
    let msgs = d["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1]["role"], "ai");
    assert_eq!(msgs[1]["content"], "你說的「正義」是什麼意思？");
    assert_eq!(msgs[1]["question_type"], "clarify");

    // AI 收到系統提示（含題目）與完整歷史，且不含結構化段落
    let reqs = s.ai.requests.lock().unwrap();
    assert!(reqs[0].system.contains("電車難題"));
    assert_eq!(reqs[0].messages.len(), 1);
}

#[sqlx::test]
async fn rules_version_is_recorded_on_ai_reply(pool: PgPool) {
    let s = setup(pool.clone(), vec![Reply(REPLY.to_vec())]).await;
    say(&s, "嗨").await;
    stream_events(&s).await;
    let (v,): (Option<String>,) =
        sqlx::query_as("SELECT rules_version FROM messages WHERE role = 'ai'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(v.as_deref(), Some(socrates_chat_backend::ai::RULES_VERSION));
}

#[sqlx::test]
async fn stage_advances_one_step_at_a_time_and_never_back(pool: PgPool) {
    let adv = || {
        Reply(vec![
            "問題？<<<META>>>{\"question_type\":\"reason\",\"advance\":true}",
        ])
    };
    let stay = || {
        Reply(vec![
            "問題？<<<META>>>{\"question_type\":\"reason\",\"advance\":false}",
        ])
    };
    let s = setup(pool, vec![adv(), stay(), adv(), adv()]).await;

    say(&s, "一").await;
    assert_eq!(stream_events(&s).await.last().unwrap().1["stage"], 2);
    say(&s, "二").await;
    assert_eq!(stream_events(&s).await.last().unwrap().1["stage"], 2); // 不退回、也不跳級
    say(&s, "三").await;
    let done = stream_events(&s).await.last().unwrap().1.clone();
    assert_eq!(done["stage"], 3);
    assert_eq!(done["suggest_end"], false);
    // 第 3 階段達成條件 → 符合收斂條件
    say(&s, "四").await;
    let done = stream_events(&s).await.last().unwrap().1.clone();
    assert_eq!(done["stage"], 3);
    assert_eq!(done["suggest_end"], true);
    assert_eq!(detail(&s).await["converge_ready"], true);
}

#[sqlx::test]
async fn bad_meta_keeps_stage_and_still_saves_reply(pool: PgPool) {
    let s = setup(pool, vec![Reply(vec!["只有文字，沒有結構化段落"])]).await;
    say(&s, "嗨").await;
    let events = stream_events(&s).await;
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(events.last().unwrap().1["stage"], 1);
    assert_eq!(detail(&s).await["messages"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn retries_once_when_nothing_was_streamed(pool: PgPool) {
    let s = setup(pool, vec![Fail, Reply(REPLY.to_vec())]).await;
    say(&s, "嗨").await;
    let events = stream_events(&s).await;
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(s.ai.request_count(), 2);
}

#[sqlx::test]
async fn gives_up_after_one_retry_and_keeps_student_message(pool: PgPool) {
    let s = setup(pool, vec![Fail, Fail, Reply(REPLY.to_vec())]).await;
    say(&s, "我的想法很長很長").await;
    let events = stream_events(&s).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "error");
    assert_eq!(events[0].1["code"], "ai_unavailable");
    assert_eq!(s.ai.request_count(), 2);

    // 學生訊息保留、沒有 AI 回覆
    let msgs = detail(&s).await["messages"].clone();
    assert_eq!(msgs.as_array().unwrap().len(), 1);
    assert_eq!(msgs[0]["content"], "我的想法很長很長");

    // 重試按鈕 = 再開一次串流，成功後補上回覆
    let events = stream_events(&s).await;
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(detail(&s).await["messages"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn mid_stream_failure_is_not_retried_and_discards_partial(pool: PgPool) {
    let s = setup(
        pool,
        vec![FailAfter(vec!["講到一半"]), Reply(REPLY.to_vec())],
    )
    .await;
    say(&s, "嗨").await;
    let events = stream_events(&s).await;
    assert_eq!(events.last().unwrap().0, "error");
    assert_eq!(s.ai.request_count(), 1);
    // 部分內容不會被保存
    assert_eq!(detail(&s).await["messages"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn first_token_timeout_counts_as_failure_and_retries(pool: PgPool) {
    let s = setup(pool, vec![Hang, Hang]).await;
    say(&s, "嗨").await;
    let events = stream_events(&s).await;
    assert_eq!(events.last().unwrap().1["code"], "ai_unavailable");
    assert_eq!(s.ai.request_count(), 2);
}

#[sqlx::test]
async fn nothing_to_reply_when_last_message_is_ai(pool: PgPool) {
    let s = setup(pool, vec![Reply(REPLY.to_vec())]).await;
    let url = format!("/api/conversations/{}/stream", s.conv);
    // 還沒有學生訊息
    let res = send(&s.app, get(&url, Some(&s.a.student))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    say(&s, "嗨").await;
    stream_events(&s).await;
    // 已經回覆過，不會重複產生
    let res = send(&s.app, get(&url, Some(&s.a.student))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "nothing_to_reply");
    assert_eq!(s.ai.request_count(), 1);
}

#[sqlx::test]
async fn stream_rejects_ended_conversation(pool: PgPool) {
    let s = setup(pool.clone(), vec![]).await;
    say(&s, "嗨").await;
    sqlx::query("UPDATE conversations SET status = 'ended'")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(
        &s.app,
        get(
            &format!("/api/conversations/{}/stream", s.conv),
            Some(&s.a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(s.ai.request_count(), 0);
}

#[sqlx::test]
async fn prompt_asks_to_wrap_up_from_configured_turn(pool: PgPool) {
    // 測試設定的 wrap_up_turn = 3
    let s = setup(
        pool,
        vec![
            Reply(REPLY.to_vec()),
            Reply(REPLY.to_vec()),
            Reply(REPLY.to_vec()),
        ],
    )
    .await;
    for t in ["一", "二", "三"] {
        say(&s, t).await;
        stream_events(&s).await;
    }
    let reqs = s.ai.requests.lock().unwrap();
    assert!(!reqs[1].system.contains("開始引導學生整理"));
    assert!(reqs[2].system.contains("開始引導學生整理"));
    // 第三次請求帶著完整歷史：學生、AI、學生、AI、學生
    assert_eq!(reqs[2].messages.len(), 5);
}

#[sqlx::test]
async fn stream_is_owner_only(pool: PgPool) {
    let s = setup(pool, vec![Reply(REPLY.to_vec())]).await;
    say(&s, "嗨").await;
    let url = format!("/api/conversations/{}/stream", s.conv);
    for cookie in [&s.a.teacher, &s.a.admin] {
        // 教師與管理者不參與討論
        assert_eq!(
            send(&s.app, get(&url, Some(cookie))).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        send(&s.app, get(&url, Some(&s.a.outsider))).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&s.app, get(&url, None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // 沒有人能觸發 AI 呼叫
    assert_eq!(s.ai.request_count(), 0);
}

#[sqlx::test]
async fn only_one_reply_stream_per_conversation(pool: PgPool) {
    let s = setup(pool.clone(), vec![Reply(REPLY.to_vec())]).await;
    say(&s, "嗨").await;
    let url = format!("/api/conversations/{}/stream", s.conv);

    // 已有進行中的回覆：第二條串流與新訊息都被擋下，且不會呼叫 AI
    sqlx::query("UPDATE conversations SET generating_since = now()")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(&s.app, get(&url, Some(&s.a.student))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "reply_in_progress");
    let res = send(
        &s.app,
        json_req(
            "POST",
            &format!("/api/conversations/{}/messages", s.conv),
            Some(&s.a.student),
            Some(json!({"content": "插話"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(s.ai.request_count(), 0);

    // 殘留的 claim（例如服務重啟）過期後可重新產生
    sqlx::query("UPDATE conversations SET generating_since = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(stream_events(&s).await.last().unwrap().0, "done");
    // 結束後釋放
    let (g,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT generating_since FROM conversations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(g.is_none());
}

#[sqlx::test]
async fn concurrent_second_stream_is_rejected_while_first_runs(pool: PgPool) {
    let s = setup(pool.clone(), vec![Hang, Hang, Reply(REPLY.to_vec())]).await;
    say(&s, "嗨").await;
    let url = format!("/api/conversations/{}/stream", s.conv);

    let (app, cookie, url1) = (s.app.clone(), s.a.student.clone(), url.clone());
    let first =
        tokio::spawn(async move { text_body(send(&app, get(&url1, Some(&cookie))).await).await });
    // 等第一條串流確實取得 claim 再送第二個請求，避免依賴固定時間
    let mut claimed = false;
    for _ in 0..200 {
        let (g,): (Option<chrono::DateTime<chrono::Utc>>,) =
            sqlx::query_as("SELECT generating_since FROM conversations")
                .fetch_one(&pool)
                .await
                .unwrap();
        if g.is_some() {
            claimed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(claimed, "first stream never claimed the conversation");
    let res = send(&s.app, get(&url, Some(&s.a.student))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // 第一條失敗收尾後釋放，再開一次可成功
    let events = parse_sse(&first.await.unwrap());
    assert_eq!(events.last().unwrap().0, "error");
    assert_eq!(stream_events(&s).await.last().unwrap().0, "done");
}

#[sqlx::test]
async fn judgement_reason_is_stored_but_never_streamed(pool: PgPool) {
    let s = setup(
        pool.clone(),
        vec![Reply(vec![
            "請說明理由？<<<META>>>{\"question_type\":\"reason\",\"advance\":false,\"reason\":\"尚未說明關鍵詞\"}",
        ])],
    )
    .await;
    say(&s, "嗨").await;
    let events = stream_events(&s).await;
    assert!(!format!("{events:?}").contains("尚未說明關鍵詞"));
    let (r,): (Option<String>,) =
        sqlx::query_as("SELECT advance_reason FROM messages WHERE role = 'ai'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(r.as_deref(), Some("尚未說明關鍵詞"));
    assert!(!detail(&s).await.to_string().contains("尚未說明關鍵詞"));
}

#[sqlx::test]
async fn failure_after_only_hidden_meta_prefix_still_retries(pool: PgPool) {
    // 只收到分隔標記的前綴就失敗：學生什麼都還沒看到，可以自動重試
    let s = setup(pool, vec![FailAfter(vec!["<<<MET"]), Reply(REPLY.to_vec())]).await;
    say(&s, "嗨").await;
    assert_eq!(stream_events(&s).await.last().unwrap().0, "done");
    assert_eq!(s.ai.request_count(), 2);
}
