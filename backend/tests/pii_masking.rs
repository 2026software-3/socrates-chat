//! F-23.4：送往 AI 服務前的個資遮蔽（S-02.6）。假 AI 記錄實際送出的請求；合成資料。

mod common;

use std::{sync::Arc, time::Duration};

use axum::http::StatusCode;
use common::Script::*;
use common::*;
use serde_json::json;
use socrates_chat_backend::ai::AiRequest;
use sqlx::PgPool;

/// 合成的個資：學號、電子郵件、手機、身分證字號。
const RAW: &str = "我是 41347064S，信箱 s@example.com，手機 0912-345-678，身分證 A123456789。";
const MASKED: &str = "我是 [STUDENT_ID]，信箱 [EMAIL]，手機 [PHONE]，身分證 [NATIONAL_ID]。";
const ORIGINALS: [&str; 4] = ["41347064S", "s@example.com", "0912-345-678", "A123456789"];

/// AI 回覆本身帶佔位詞：照原樣顯示、不還原。
const REPLY: [&str; 2] = [
    "你提到 [PHONE]，但為什麼要拉桿？\n<<<META>>>\n",
    "{\"question_type\":\"reason\",\"advance\":false,\"reason\":\"尚未說明理由\"}",
];

fn sent_text(req: &AiRequest) -> String {
    let mut text = req.system.clone();
    for turn in &req.messages {
        text.push('\n');
        text.push_str(&turn.content);
    }
    text
}

fn assert_no_originals(reqs: &[AiRequest]) {
    for req in reqs {
        let text = sent_text(req);
        for original in ORIGINALS {
            assert!(!text.contains(original), "{original} was sent to the AI");
        }
    }
}

async fn wait_for_requests(ai: &Arc<FakeAi>, n: usize) {
    for _ in 0..200 {
        if ai.request_count() >= n {
            // 再給背景任務一點時間把結果寫進資料庫
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("AI was not called {n} times");
}

#[sqlx::test]
async fn typed_and_spoken_messages_are_masked_only_on_the_way_out(pool: PgPool) {
    let ai = FakeAi::with((0..3).map(|_| Reply(REPLY.to_vec())).collect());
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    // 語音逐字稿（瀏覽器辨識或 OpenAI 備援）與一般訊息走同一條路徑
    for source in ["text", "speech-browser", "speech-openai"] {
        let res = send(
            &app,
            json_req(
                "POST",
                &format!("/api/conversations/{conv}/messages"),
                Some(&a.student),
                Some(json!({"content": RAW, "source": source})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let events = parse_sse(&text_body(res).await);
        // 回給學生的仍是原文
        assert_eq!(events[0].1["content"], RAW);
        assert_eq!(events.last().unwrap().0, "done");
    }

    let reqs = ai.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3);
    for req in &reqs {
        assert_eq!(req.messages.last().unwrap().content, MASKED);
    }
    assert_no_originals(&reqs);
    // 系統提示說明佔位詞
    assert!(reqs[0].system.contains("[NATIONAL_ID]"));

    // 資料庫與學生畫面維持原文；AI 回覆中的佔位詞照原樣保存與顯示
    let stored: Vec<(String,)> =
        sqlx::query_as("SELECT content FROM messages WHERE role = 'student' ORDER BY created_at")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(stored.iter().all(|(c,)| c == RAW));
    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    let msgs = d["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["content"], RAW);
    assert_eq!(msgs[1]["content"], "你提到 [PHONE]，但為什麼要拉桿？");
}

#[sqlx::test]
async fn summary_and_claim_grouping_requests_are_masked(pool: PgPool) {
    let summary = json!({
        "stance": "會拉桿", "reasons": "五個人比一個人多",
        "claim": "s@example.com 認為該拉桿",
        "framework": "utilitarianism",
        "scores": {"utilitarianism": 4, "deontology": 1, "virtue": 2,
                   "contractarianism": 1, "care": 2, "existentialism": 0},
    })
    .to_string();
    let ai = FakeAi::with(vec![
        Reply(vec![Box::leak(summary.into_boxed_str())]),
        Reply(vec!["{\"group\":\"拉桿\"}"]),
        Reply(vec!["{\"groups\":[{\"name\":\"拉桿\",\"members\":[1]}]}"]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, RAW).await;

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    // 總結，再接著主張分組
    wait_for_requests(&ai, 2).await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            Some(&a.teacher),
            Some(json!({"title": "電車難題"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let reqs = ai.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3);
    assert!(sent_text(&reqs[0]).contains(MASKED), "總結的逐字稿已遮蔽");
    assert!(
        sent_text(&reqs[1]).contains("[EMAIL] 認為該拉桿"),
        "歸群的主張已遮蔽"
    );
    assert!(
        sent_text(&reqs[2]).contains("[EMAIL] 認為該拉桿"),
        "重新分組的主張已遮蔽"
    );
    assert_no_originals(&reqs);
}
