//! F-06.2 / F-06.4 / F-07：AI 回覆串流、重試、逾時、階段推進與授權（假 AI、合成資料）。

mod common;

use std::sync::Arc;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::config::Config;
use sqlx::PgPool;

struct Setup {
    app: Router,
    a: Actors,
    ai: Arc<FakeAi>,
    conv: String,
}

async fn setup(pool: PgPool, scripts: Vec<Script>) -> Setup {
    setup_with_config(pool, scripts, test_config()).await
}

async fn setup_with_config(pool: PgPool, scripts: Vec<Script>, config: Config) -> Setup {
    let ai = FakeAi::with(scripts);
    let app = test_app_with_config(pool, ai.clone(), config);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    Setup { app, a, ai, conv }
}

/// 送出學生訊息並讀完回應（SSE）：第一個事件是保存好的學生訊息，其餘（delta／done／error）回傳。
async fn say(s: &Setup, text: &str) -> Vec<(String, Value)> {
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
    assert_eq!(res.status(), StatusCode::OK);
    let mut events = parse_sse(&text_body(res).await);
    let (name, saved) = events.remove(0);
    assert_eq!(name, "message");
    assert_eq!(saved["role"], "student");
    assert_eq!(saved["content"], text);
    assert_eq!(saved["auto_ended"], false);
    events
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
    let events = say(&s, "我認為應該拉桿。").await;

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
    let (v,): (Option<String>,) =
        sqlx::query_as("SELECT rules_version FROM messages WHERE role = 'ai'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(v.as_deref(), Some(socrates_chat_backend::ai::RULES_VERSION));
}

#[sqlx::test]
async fn english_conversation_uses_an_english_prompt_and_version(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![
        "What do you mean?<<<META>>>{\"question_type\":\"clarify\",\"advance\":false}",
    ])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let updated = send(
        &app,
        json_req(
            "PATCH",
            "/api/me",
            Some(&a.student),
            Some(json!({"language": "en"})),
        ),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let conv = start_conversation(&app, &a).await;
    let setup = Setup { app, a, ai, conv };

    say(&setup, "I would pull the lever.").await;

    {
        let requests = setup.ai.requests.lock().unwrap();
        assert!(
            requests[0]
                .system
                .contains("Respond to the student only in English")
        );
    }
    let version: String =
        sqlx::query_scalar("SELECT rules_version FROM messages WHERE role = 'ai'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(version, "v1.14/en");
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

    assert_eq!(say(&s, "一").await.last().unwrap().1["stage"], 2);
    assert_eq!(say(&s, "二").await.last().unwrap().1["stage"], 2); // 不退回、也不跳級
    let done = say(&s, "三").await.last().unwrap().1.clone();
    assert_eq!(done["stage"], 3);
    assert_eq!(done["suggest_end"], false);
    // 第 3 階段達成條件 → 符合收斂條件
    let done = say(&s, "四").await.last().unwrap().1.clone();
    assert_eq!(done["stage"], 3);
    assert_eq!(done["suggest_end"], true);
    assert_eq!(detail(&s).await["converge_ready"], true);
}

#[sqlx::test]
async fn bad_meta_keeps_stage_and_still_saves_reply(pool: PgPool) {
    let s = setup(pool, vec![Reply(vec!["只有文字，沒有結構化段落"])]).await;
    let events = say(&s, "嗨").await;
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(events.last().unwrap().1["stage"], 1);
    assert_eq!(detail(&s).await["messages"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn retries_once_when_nothing_was_streamed(pool: PgPool) {
    let s = setup(pool, vec![Fail, Reply(REPLY.to_vec())]).await;
    let events = say(&s, "嗨").await;
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(s.ai.request_count(), 2);
}

#[sqlx::test]
async fn gives_up_after_one_retry_and_keeps_student_message(pool: PgPool) {
    let s = setup(pool, vec![Fail, Fail, Reply(REPLY.to_vec())]).await;
    let events = say(&s, "我的想法很長很長").await;
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
    let events = say(&s, "嗨").await;
    assert_eq!(events.last().unwrap().0, "error");
    assert_eq!(s.ai.request_count(), 1);
    // 部分內容不會被保存
    assert_eq!(detail(&s).await["messages"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn first_token_timeout_counts_as_failure_and_retries(pool: PgPool) {
    let s = setup(pool, vec![Hang, Hang]).await;
    let events = say(&s, "嗨").await;
    assert_eq!(events.last().unwrap().1["code"], "ai_unavailable");
    assert_eq!(s.ai.request_count(), 2);
}

#[sqlx::test]
async fn reasoning_before_the_first_word_does_not_trip_the_first_token_timeout(pool: PgPool) {
    // 空片段代表模型開始推理（OpenAiProvider 送出）；首字逾時 200 ms，正文在 300 ms 才出現
    let paced = [""].into_iter().chain(REPLY).collect();
    let s = setup(
        pool,
        vec![Paced(paced, std::time::Duration::from_millis(150))],
    )
    .await;
    let events = say(&s, "嗨").await;
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(s.ai.request_count(), 1);
    // 空片段不會變成給學生的 delta
    assert!(
        events
            .iter()
            .filter(|(n, _)| n == "delta")
            .all(|(_, d)| !d["text"].as_str().unwrap().is_empty())
    );
}

#[sqlx::test]
async fn nothing_to_reply_when_last_message_is_ai(pool: PgPool) {
    let s = setup(pool, vec![Reply(REPLY.to_vec())]).await;
    let url = format!("/api/conversations/{}/stream", s.conv);
    // 還沒有學生訊息
    let res = send(&s.app, get(&url, Some(&s.a.student))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    say(&s, "嗨").await;
    // 已經回覆過，不會重複產生
    let res = send(&s.app, get(&url, Some(&s.a.student))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "nothing_to_reply");
    assert_eq!(s.ai.request_count(), 1);
}

#[sqlx::test]
async fn stream_rejects_ended_conversation(pool: PgPool) {
    let s = setup(pool.clone(), vec![]).await;
    seed_student_message(&pool, &s.conv, "嗨").await;
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
    }
    let reqs = s.ai.requests.lock().unwrap();
    assert!(!reqs[1].system.contains("開始引導學生整理"));
    assert!(reqs[2].system.contains("開始引導學生整理"));
    // 第三次請求帶著完整歷史：學生、AI、學生、AI、學生
    assert_eq!(reqs[2].messages.len(), 5);
}

#[sqlx::test]
async fn stream_is_owner_only(pool: PgPool) {
    let s = setup(pool.clone(), vec![Reply(REPLY.to_vec())]).await;
    seed_student_message(&pool, &s.conv, "嗨").await;
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
    seed_student_message(&pool, &s.conv, "嗨").await;
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
async fn second_stream_joins_the_running_reply_instead_of_starting_another(pool: PgPool) {
    // 兩次首字逾時就是第一個回覆的長度：放寬到第二條串流在平行執行時也來得及接上
    let config = Config {
        ai_first_token_timeout: std::time::Duration::from_secs(1),
        ..test_config()
    };
    let s = setup_with_config(
        pool.clone(),
        vec![Hang, Hang, Reply(REPLY.to_vec())],
        config,
    )
    .await;
    seed_student_message(&pool, &s.conv, "嗨").await;
    let url = format!("/api/conversations/{}/stream", s.conv);

    let (app, cookie, url1) = (s.app.clone(), s.a.student.clone(), url.clone());
    let first =
        tokio::spawn(async move { text_body(send(&app, get(&url1, Some(&cookie))).await).await });
    wait_until_generating(&s).await;
    // 第二條串流接上同一個回覆：收到相同的結果，也不會多呼叫一次 AI
    let second = text_body(send(&s.app, get(&url, Some(&s.a.student))).await).await;
    let events = parse_sse(&first.await.unwrap());
    assert_eq!(events.last().unwrap().0, "error");
    assert_eq!(parse_sse(&second), events);
    assert_eq!(s.ai.request_count(), 2);

    // 失敗收尾後釋放，再開一次會重新產生
    assert_eq!(stream_events(&s).await.last().unwrap().0, "done");
}

/// 等到對話詳情顯示回覆正在產生（`generating`），避免依賴固定時間。
async fn wait_until_generating(s: &Setup) {
    for _ in 0..200 {
        if detail(s).await["generating"] == true {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("reply never started generating");
}

#[sqlx::test]
async fn leaving_mid_reply_and_coming_back_resumes_the_same_reply(pool: PgPool) {
    // 回覆要撐過「斷線、查詢、重新接上」這段時間；測試平行執行時很慢，片段間隔放寬，逾時跟著放寬
    let pace = std::time::Duration::from_millis(400);
    let config = Config {
        ai_first_token_timeout: std::time::Duration::from_secs(2),
        ai_total_timeout: std::time::Duration::from_secs(5),
        ..test_config()
    };
    let s = setup_with_config(pool.clone(), vec![Paced(REPLY.to_vec(), pace)], config).await;
    assert_eq!(detail(&s).await["generating"], false);

    // 送出訊息、收到第一段回覆後就離開頁面（中斷連線）
    let res = send(
        &s.app,
        json_req(
            "POST",
            &format!("/api/conversations/{}/messages", s.conv),
            Some(&s.a.student),
            Some(json!({"content": "嗨"})),
        ),
    )
    .await;
    let mut body = res.into_body().into_data_stream();
    let mut seen = String::new();
    while !seen.contains("event: delta") {
        let chunk = futures_util::StreamExt::next(&mut body)
            .await
            .unwrap()
            .unwrap();
        seen.push_str(std::str::from_utf8(&chunk).unwrap());
    }
    drop(body);

    // 回來時回覆還在產生：重新接上，先補上已產生的文字，再收到剩下的片段與 done
    assert_eq!(detail(&s).await["generating"], true);
    let events = stream_events(&s).await;
    let text: String = events
        .iter()
        .filter(|(name, _)| name == "delta")
        .map(|(_, v)| v["text"].as_str().unwrap())
        .collect();
    // 與一路沒有斷線的連線收到的文字相同（串流不去掉結尾空白，保存時才去掉）
    assert_eq!(text, "你說的「正義」是什麼意思？\n");
    assert_eq!(events.last().unwrap().0, "done");
    assert_eq!(s.ai.request_count(), 1);

    let d = detail(&s).await;
    assert_eq!(d["generating"], false);
    assert_eq!(d["messages"][1]["content"], "你說的「正義」是什麼意思？");
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
    let events = say(&s, "嗨").await;
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
    assert_eq!(say(&s, "嗨").await.last().unwrap().0, "done");
    assert_eq!(s.ai.request_count(), 2);
}

#[sqlx::test]
async fn posting_a_message_holds_the_claim_until_the_reply_finishes(pool: PgPool) {
    let s = setup(pool.clone(), vec![Hang, Hang]).await;
    let url = format!("/api/conversations/{}/messages", s.conv);

    let (app, cookie, url1) = (s.app.clone(), s.a.student.clone(), url.clone());
    let first = tokio::spawn(async move {
        let req = json_req("POST", &url1, Some(&cookie), Some(json!({"content": "嗨"})));
        text_body(send(&app, req).await).await
    });
    // 等第一則訊息的串流確實取得 claim，避免依賴固定時間
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
    assert!(claimed, "POST never claimed the conversation");

    // 第一則的回覆還在產生：第二則被擋下，且不計回合、不保存
    let res = send(
        &s.app,
        json_req(
            "POST",
            &url,
            Some(&s.a.student),
            Some(json!({"content": "插話"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "reply_in_progress");

    let events = parse_sse(&first.await.unwrap());
    assert_eq!(events[0].0, "message");
    assert_eq!(events.last().unwrap().1["code"], "ai_unavailable");
    let d = detail(&s).await;
    assert_eq!(d["messages"].as_array().unwrap().len(), 1);
    assert_eq!(d["turn_count"], 1);
    // 串流結束後釋放
    let (g,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT generating_since FROM conversations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(g.is_none());
}
