//! F-08 結束與總結、F-26 教師檢視：行為、失敗處理與授權（假 AI、合成資料）。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::config::Config;
use sqlx::PgPool;

const GOOD: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";
const INCOMPLETE: &str = "{\"stance\":\"會拉桿\"}";

/// 輪詢到總結不再是 pending 為止（背景產生）。
async fn settled_summary(app: &Router, cookie: &str, conv: &str) -> Value {
    for _ in 0..100 {
        let res = send(
            app,
            get(&format!("/api/conversations/{conv}/summary"), Some(cookie)),
        )
        .await;
        let v = json_body(res).await;
        if v["status"] != "pending" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("summary stayed pending");
}

async fn end(app: &Router, cookie: &str, conv: &str) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(cookie),
            None,
        ),
    )
    .await;
    let s = res.status();
    (s, json_body(res).await)
}

#[sqlx::test]
async fn ending_generates_a_saved_summary(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "我會拉桿，因為五個人比一個人多。").await;

    let (status, body) = end(&app, &a.student, &conv).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["status"], "pending");

    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "ready");
    assert_eq!(s["stance"], "會拉桿");
    assert_eq!(s["reasons"], "救五人優先");

    // 對話已結束，不能再送訊息；AI 收到的是完整逐字稿
    let (st, _) = student_says(&app, &a, &conv, "還想補充").await;
    assert_eq!(st, StatusCode::CONFLICT);
    let reqs = ai.requests.lock().unwrap();
    assert!(reqs[0].messages[0].content.contains("學生：我會拉桿"));
    assert!(reqs[0].system.contains("電車難題"));
}

#[sqlx::test]
async fn spanish_conversation_uses_spanish_summary_prompt_transcript_and_version(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![
        "{\"stance\":\"Accionaría la palanca\",\"reasons\":\"Salvar a cinco personas\",\"claim\":\"Accionar la palanca\"}",
    ])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let updated = send(
        &app,
        json_req(
            "PATCH",
            "/api/me",
            Some(&a.student),
            Some(json!({"language": "es"})),
        ),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "Accionaría la palanca.").await;

    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    for _ in 0..100 {
        if ai.request_count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(ai.request_count(), 2, "claim grouping did not call the AI");

    {
        let requests = ai.requests.lock().unwrap();
        assert!(
            requests[0]
                .system
                .contains("Escribe el resumen únicamente en español")
        );
        assert!(
            requests[0].messages[0]
                .content
                .contains("Estudiante: Accionaría la palanca.")
        );
        // 群組名稱給教室擁有者（這裡是預設的繁體中文教師）看，不跟著學生的對話語言
        assert!(requests[1].system.contains("只使用繁體中文"));
        assert!(!requests[1].system.contains("Agrupa la afirmación"));
    }
    let version: String =
        sqlx::query_scalar("SELECT rules_version FROM summaries WHERE conversation_id = $1")
            .bind(uuid::Uuid::parse_str(&conv).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(version, "v1.21/es");
}

#[sqlx::test]
async fn cannot_end_an_empty_conversation_and_ending_twice_is_idempotent(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD]), Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    let (st, body) = end(&app, &a.student, &conv).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "empty_conversation");

    seed_student_message(&pool, &conv, "想法").await;
    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::ACCEPTED);
    settled_summary(&app, &a.student, &conv).await;
    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::OK);
    assert_eq!(ai.request_count(), 1, "不會重複產生總結");
}

#[sqlx::test]
async fn failed_summary_is_marked_and_can_be_retried(pool: PgPool) {
    // 第一輪（含自動重試）兩次都失敗，之後手動重試成功
    let ai = FakeAi::with(vec![Fail, Fail, Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;

    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "failed");
    assert!(s["stance"].is_null());
    assert_eq!(ai.request_count(), 2);

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );

    // 完成後不能再重試
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "summary_not_retryable"
    );
}

#[sqlx::test]
async fn incomplete_ai_result_counts_as_failure(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![INCOMPLETE]), Reply(vec!["不是 JSON"])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "failed");
    assert!(s["stance"].is_null() && s["reasons"].is_null());
}

#[sqlx::test]
async fn first_attempt_incomplete_then_complete_is_ready(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![INCOMPLETE]), Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
}

#[sqlx::test]
async fn stale_pending_summary_can_be_retried_but_fresh_one_cannot(pool: PgPool) {
    let ai = FakeAi::with(vec![Hang, Hang, Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    sqlx::query("UPDATE summaries SET status = 'pending', started_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let retry = || {
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        )
    };
    assert_eq!(send(&app, retry()).await.status(), StatusCode::CONFLICT);
    sqlx::query("UPDATE summaries SET started_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(send(&app, retry()).await.status(), StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn reaching_turn_limit_ends_conversation_and_summarises(pool: PgPool) {
    // 測試設定的 max_turns = 5
    // 前 4 則各觸發一次 AI 回覆，第 5 則直接結束並產生總結
    let reply = || Reply(vec!["然後呢？"]);
    let ai = FakeAi::with(vec![reply(), reply(), reply(), reply(), Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    for i in 1..5 {
        let (st, body) = student_says(&app, &a, &conv, &format!("第{i}則")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["auto_ended"], false);
    }
    let (st, body) = student_says(&app, &a, &conv, "第5則").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["auto_ended"], true);
    assert_eq!(body["content"], "第5則");

    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(d["status"], "ended");
    assert_eq!(
        student_says(&app, &a, &conv, "第6則").await.0,
        StatusCode::CONFLICT
    );
}

#[sqlx::test]
async fn summary_endpoints_are_owner_only_and_read_only(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    for cookie in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            get(&format!("/api/conversations/{conv}/summary"), Some(cookie)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(end(&app, cookie, &conv).await.0, StatusCode::FORBIDDEN);
        let res = send(
            &app,
            json_req(
                "POST",
                &format!("/api/conversations/{conv}/summary/retry"),
                Some(cookie),
                None,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
    let res = send(
        &app,
        get(
            &format!("/api/conversations/{conv}/summary"),
            Some(&a.outsider),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}/summary"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 學生不能修改總結：沒有任何寫入端點
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/conversations/{conv}/summary"),
            Some(&a.student),
            Some(json!({"stance": "改掉"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["stance"], "會拉桿");
}

#[sqlx::test]
async fn teacher_sees_identified_summaries_but_never_message_text(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "這句是私密的對話原文").await;
    // 另一場尚未結束、以及一場失敗的，都不該出現
    let _active = start_conversation(&app, &a).await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let res = send(&app, get("/api/summaries", Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = json_body(res).await["items"].clone();
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["conversation_id"], conv);
    assert_eq!(rows[0]["student_email"], "student@example.com");
    assert_eq!(rows[0]["student_name"], "學生");
    assert_eq!(rows[0]["language"], "zh-TW");
    assert_eq!(rows[0]["stance"], "會拉桿");
    assert!(!list.to_string().contains("私密的對話原文"));

    // 學生被移出名單後，教師仍看得到過去的總結（S-01.4）
    send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{}/roster/student@example.com", a.classroom),
            Some(&a.admin),
            None,
        ),
    )
    .await;
    let list =
        json_body(send(&app, get("/api/summaries", Some(&a.teacher))).await).await["items"].clone();
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn teacher_translates_a_summary_into_their_account_language_without_conversation_text(
    pool: PgPool,
) {
    let translated = "{\"stance\":\"Would pull the lever\",\"reasons\":\"Save more people\"}";
    let ai = FakeAi::with(vec![Reply(vec![GOOD]), Reply(vec![translated])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "這句是私密的對話原文").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;
    let updated = send(
        &app,
        json_req(
            "PATCH",
            "/api/me",
            Some(&a.teacher),
            Some(json!({"language": "en"})),
        ),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);

    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/summaries/{conv}/translation"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "language": "en",
            "stance": "Would pull the lever",
            "reasons": "Save more people",
        })
    );

    let requests = ai.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let translation = &requests[1];
    assert!(
        translation
            .system
            .contains("Translate the summary into English")
    );
    assert!(translation.messages[0].content.contains("會拉桿"));
    assert!(translation.messages[0].content.contains("救五人優先"));
    for private in ["這句是私密的對話原文", "student@example.com", "學生"] {
        assert!(!translation.messages[0].content.contains(private));
    }
}

#[sqlx::test]
async fn summary_translation_keeps_teacher_scope_and_admin_masking_boundaries(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD]), Fail]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;
    let path = format!("/api/summaries/{conv}/translation");

    for (cookie, expected) in [
        (Some(a.admin.as_str()), StatusCode::FORBIDDEN),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(&app, json_req("POST", &path, cookie, None))
                .await
                .status(),
            expected
        );
    }

    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email": "other-teacher@example.com"})),
        ),
    )
    .await;
    let other = login_as(&app, "sub-other-teacher", "other-teacher@example.com").await;
    assert_eq!(
        send(&app, json_req("POST", &path, Some(&other), None))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    send(
        &app,
        json_req(
            "PATCH",
            "/api/me",
            Some(&a.teacher),
            Some(json!({"language": "en"})),
        ),
    )
    .await;
    let failed = send(&app, json_req("POST", &path, Some(&a.teacher), None)).await;
    assert_eq!(failed.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(json_body(failed).await["error"]["code"], "ai_unavailable");
    assert_eq!(ai.request_count(), 2, "拒絕的請求不得呼叫 AI");
}

#[sqlx::test]
async fn admin_view_of_summaries_is_masked(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let list =
        json_body(send(&app, get("/api/summaries", Some(&a.admin))).await).await["items"].clone();
    let row = &list[0];
    for f in ["stance", "reasons"] {
        assert_eq!(row[f], "message");
    }
    assert!(!list.to_string().contains("會拉桿"));
}

#[sqlx::test]
async fn teacher_summary_list_denies_everyone_else(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (cookie, expected) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(&app, get("/api/summaries", cookie)).await.status(),
            expected
        );
    }
}

#[sqlx::test]
async fn deleting_conversation_removes_summary_from_teacher_view(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{conv}"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let list =
        json_body(send(&app, get("/api/summaries", Some(&a.teacher))).await).await["items"].clone();
    assert!(list.as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn admin_who_is_also_teacher_still_sees_masked_summaries(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    // 管理者同時被加為教師
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email": "admin@example.com"})),
        ),
    )
    .await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let list =
        json_body(send(&app, get("/api/summaries", Some(&a.admin))).await).await["items"].clone();
    assert_eq!(list[0]["stance"], "message");
    assert!(!list.to_string().contains("會拉桿"));
}

#[sqlx::test]
async fn teacher_list_only_contains_student_conversations(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    // 教師不參與討論：不能開對話
    let act = published_activity_with_topic(&app, &a, "試用").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.teacher),
            Some(json!({"activity_id": act})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 學生的對話才會出現
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let list =
        json_body(send(&app, get("/api/summaries", Some(&a.teacher))).await).await["items"].clone();
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["conversation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![conv.as_str()]);
}

#[sqlx::test]
async fn ending_waits_for_an_in_flight_ai_reply(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;

    sqlx::query("UPDATE conversations SET generating_since = now()")
        .execute(&pool)
        .await
        .unwrap();
    let (st, body) = end(&app, &a.student, &conv).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "reply_in_progress");
    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(d["status"], "active");

    // 回覆完成（claim 釋放）後即可結束
    sqlx::query("UPDATE conversations SET generating_since = NULL")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn superseded_generation_cannot_overwrite_the_newer_one(pool: PgPool) {
    // 第一代卡住（Hang）；在它逾時前手動重試，第二代很快成功。
    // 第一代之後失敗收尾時，不能把已完成的結果改成 failed。
    // 總結只受完整回覆上限限制；這個上限要遠大於手動重試的等待，否則第一代的自動重試會先拿走第二個腳本
    let config = Config {
        ai_total_timeout: Duration::from_millis(1500),
        ..test_config()
    };
    let ai = FakeAi::with(vec![Hang, Reply(vec![GOOD])]);
    let app = test_app_with_config(pool.clone(), ai.clone(), config);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    // 等第一代真的拿到 Hang 腳本，重試的第二代才會拿到 Reply
    while ai.request_count() < 1 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    sqlx::query("UPDATE summaries SET started_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );

    // 等第一代的逾時與自動重試都結束
    tokio::time::sleep(Duration::from_millis(1700)).await;
    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "ready");
    assert_eq!(s["stance"], "會拉桿");
}

#[sqlx::test]
async fn orphaned_pending_summaries_are_failed_on_startup(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    sqlx::query("UPDATE conversations SET status = 'ended'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO summaries (conversation_id) VALUES ($1::uuid)")
        .bind(&conv)
        .execute(&pool)
        .await
        .unwrap();
    let n = socrates_chat_backend::summary::recover_orphaned(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "failed"
    );
}

/// 學生結束一場繁體中文對話並等總結完成，教師的語言改成英文，回傳 (app, actors, 對話 id, 翻譯路徑)。
async fn translation_fixture(
    pool: PgPool,
    ai: std::sync::Arc<FakeAi>,
    limit: Option<(usize, Duration)>,
) -> (Router, Actors, String) {
    translation_fixture_with_config(pool, ai, limit, test_config()).await
}

async fn translation_fixture_with_config(
    pool: PgPool,
    ai: std::sync::Arc<FakeAi>,
    limit: Option<(usize, Duration)>,
    config: Config,
) -> (Router, Actors, String) {
    register_fixture_pool(&pool);
    let mut state = socrates_chat_backend::AppState::new(
        pool.clone(),
        config,
        std::sync::Arc::new(FakeIdentity),
        ai,
    );
    if let Some((max, window)) = limit {
        state = state.with_translation_limit(max, window);
    }
    let app = socrates_chat_backend::app(state);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;
    (app, a, conv)
}

async fn set_teacher_language(app: &Router, a: &Actors, language: &str) {
    let res = send(
        app,
        json_req(
            "PATCH",
            "/api/me",
            Some(&a.teacher),
            Some(json!({"language": language})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

async fn translate(app: &Router, a: &Actors, conv: &str) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/summaries/{conv}/translation"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

fn translation_reply(reasons_chars: usize) -> &'static str {
    let body = json!({"stance": "Would pull", "reasons": "x".repeat(reasons_chars)});
    Box::leak(body.to_string().into_boxed_str())
}

#[sqlx::test]
async fn translation_is_not_cut_off_by_the_general_request_timeout(pool: PgPool) {
    // 翻譯同步等一次 AI 呼叫，推理模型常常比一般請求逾時慢得多；要用長逾時，不能回 408
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Paced(vec![translation_reply(10)], Duration::from_millis(500)),
    ]);
    let (app, a, conv) = translation_fixture_with_config(pool, ai, None, short_timeout()).await;
    set_teacher_language(&app, &a, "en").await;
    let (status, body) = translate(&app, &a, &conv).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stance"], "Would pull");
}

#[sqlx::test]
async fn translation_that_grows_past_the_summary_limit_is_not_silently_truncated(pool: PgPool) {
    // 2000 字的中文理由翻成拉丁字母常常超過 2000 字元；不能被總結欄位的上限悄悄截斷
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Reply(vec![translation_reply(3000)]),
    ]);
    let (app, a, conv) = translation_fixture(pool, ai, None).await;
    set_teacher_language(&app, &a, "en").await;
    let (status, body) = translate(&app, &a, &conv).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["reasons"].as_str().unwrap().chars().count(), 3000);
}

#[sqlx::test]
async fn translation_over_the_hard_cap_fails_instead_of_returning_a_cut_text(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Reply(vec![translation_reply(9000)]),
    ]);
    let (app, a, conv) = translation_fixture(pool, ai, None).await;
    set_teacher_language(&app, &a, "en").await;
    let (status, body) = translate(&app, &a, &conv).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "ai_unavailable");
}

#[sqlx::test]
async fn translation_requests_are_rate_limited_per_teacher(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Reply(vec![translation_reply(10)]),
        Reply(vec![translation_reply(10)]),
        Reply(vec![translation_reply(10)]),
    ]);
    let (app, a, conv) =
        translation_fixture(pool, ai.clone(), Some((2, Duration::from_secs(600)))).await;

    // 語言相同不會呼叫 AI，不吃額度
    for _ in 0..5 {
        assert_eq!(translate(&app, &a, &conv).await.0, StatusCode::OK);
    }
    assert_eq!(ai.request_count(), 1);

    set_teacher_language(&app, &a, "en").await;
    assert_eq!(translate(&app, &a, &conv).await.0, StatusCode::OK);
    assert_eq!(translate(&app, &a, &conv).await.0, StatusCode::OK);
    let (status, body) = translate(&app, &a, &conv).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_many_translations");
    assert_eq!(ai.request_count(), 3, "超過額度的請求不得呼叫 AI");
}

#[sqlx::test]
async fn a_failed_translation_does_not_use_up_the_teachers_quota(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Fail,                                 // AI 逾時
        Reply(vec!["{\"stance\":\"x\"}"]),    // 格式不完整
        Reply(vec![translation_reply(9000)]), // 譯文過長
        Reply(vec![translation_reply(10)]),
    ]);
    let (app, a, conv) = translation_fixture(pool, ai, Some((1, Duration::from_secs(600)))).await;
    set_teacher_language(&app, &a, "en").await;

    // 額度只有 1：三次失敗都不能吃掉它，之後的成功請求仍然可以通過
    for _ in 0..3 {
        assert_eq!(translate(&app, &a, &conv).await.0, StatusCode::BAD_GATEWAY);
    }
    assert_eq!(translate(&app, &a, &conv).await.0, StatusCode::OK);
    // 成功的那次才算數
    assert_eq!(
        translate(&app, &a, &conv).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}
