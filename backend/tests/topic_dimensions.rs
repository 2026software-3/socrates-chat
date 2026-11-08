mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const GENERATED: &str = r#"[{"name":"Conservation","description":"How a decision handles conservation.","rubric":["Evidence 1 for criterion 1","Evidence 2 for criterion 1","Evidence 3 for criterion 1","Evidence 4 for criterion 1","Evidence 5 for criterion 1"]},{"name":"Fairness","description":"How a decision handles fairness.","rubric":["Evidence 1 for criterion 2","Evidence 2 for criterion 2","Evidence 3 for criterion 2","Evidence 4 for criterion 2","Evidence 5 for criterion 2"]},{"name":"Urgency","description":"How a decision handles urgency.","rubric":["Evidence 1 for criterion 3","Evidence 2 for criterion 3","Evidence 3 for criterion 3","Evidence 4 for criterion 3","Evidence 5 for criterion 3"]},{"name":"Consultation","description":"How a decision handles consultation.","rubric":["Evidence 1 for criterion 4","Evidence 2 for criterion 4","Evidence 3 for criterion 4","Evidence 4 for criterion 4","Evidence 5 for criterion 4"]},{"name":"Long-term planning","description":"How a decision handles long-term planning.","rubric":["Evidence 1 for criterion 5","Evidence 2 for criterion 5","Evidence 3 for criterion 5","Evidence 4 for criterion 5","Evidence 5 for criterion 5"]},{"name":"Transparency","description":"How a decision handles transparency.","rubric":["Evidence 1 for criterion 6","Evidence 2 for criterion 6","Evidence 3 for criterion 6","Evidence 4 for criterion 6","Evidence 5 for criterion 6"]}]"#;
const EN: &str = r#"["Conservation","Conserve water","Fairness","Share fairly","Urgency","Act now","Consultation","Ask affected people","Planning","Plan ahead","Transparency","Explain decisions"]"#;
const ES: &str = r#"["Conservación","Conservar agua","Equidad","Distribuir con equidad","Urgencia","Actuar ya","Consulta","Escuchar a los afectados","Planificación","Planificar a largo plazo","Transparencia","Explicar las decisiones"]"#;

async fn create(app: &axum::Router, cookie: &str) -> String {
    let response = send(
        app,
        json_req(
            "POST",
            "/api/topics",
            Some(cookie),
            Some(json!({"title": "Who should allocate a scarce water supply?"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["id"].as_str().unwrap().to_owned()
}

#[sqlx::test]
async fn creating_a_topic_attempts_generation_and_keeps_the_topic_when_ai_fails(pool: PgPool) {
    let ai = FakeAi::with(vec![Script::Fail, Script::Fail]);
    let app = test_app_with_ai(pool, ai.clone());
    let actors = seed_actors(&app).await;
    let id = create(&app, &actors.teacher).await;
    assert!(!ai.requests.lock().unwrap().is_empty());
    let result = json_body(
        send(
            &app,
            get(
                &format!("/api/topics/{id}/dimensions"),
                Some(&actors.teacher),
            ),
        )
        .await,
    )
    .await;
    assert!(result["dimensions"].is_null());
}

#[sqlx::test]
async fn retry_persists_a_complete_translated_set_and_cannot_replace_it(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Script::Reply(vec!["[]"]),
        Script::Fail,
        Script::Reply(vec![GENERATED]),
        Script::Reply(vec![EN]),
        Script::Reply(vec![ES]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let actors = seed_actors(&app).await;
    let id = create(&app, &actors.teacher).await;
    let path = format!("/api/topics/{id}/dimensions");
    let retry = format!("{path}/retry");
    let result = send(&app, json_req("POST", &retry, Some(&actors.teacher), None)).await;
    assert_eq!(result.status(), StatusCode::OK);
    let view = json_body(result).await;
    let dimensions = view["dimensions"].as_array().unwrap();
    assert_eq!(
        dimensions
            .iter()
            .map(|d| d["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["d1", "d2", "d3", "d4", "d5", "d6"]
    );
    assert_eq!(dimensions[0]["name"]["en"], "Conservation");
    assert_eq!(dimensions[0]["name"]["es"], "Conservación");
    assert_eq!(dimensions[0]["rubric"].as_array().unwrap().len(), 5);
    assert_eq!(view["pending_languages"], json!([]));
    assert_eq!(
        view["prompt_version"],
        socrates_chat_backend::topic_dimensions::PROMPT_VERSION
    );
    let persisted: Value = sqlx::query_scalar("SELECT dimensions FROM topics WHERE id = $1")
        .bind(Uuid::parse_str(&id).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(persisted, view["dimensions"]);
    let repeated =
        json_body(send(&app, json_req("POST", &retry, Some(&actors.teacher), None)).await).await;
    assert_eq!(repeated, view);
    assert_eq!(ai.requests.lock().unwrap().len(), 5);
    sqlx::migrate!().run(&pool).await.unwrap();
}

#[sqlx::test]
async fn edits_are_localized_and_a_conversation_permanently_locks_the_rubric(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Script::Reply(vec![GENERATED]),
        Script::Fail,
        Script::Fail,
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let actors = seed_actors(&app).await;
    let id = create(&app, &actors.teacher).await;
    let path = format!("/api/topics/{id}/dimensions");
    let view = json_body(send(&app, get(&path, Some(&actors.teacher))).await).await;
    let mut edits = view["dimensions"].as_array().unwrap().iter().map(|item| json!({
        "id": item["id"], "name": item["name"]["en"], "description": item["description"]["en"],
        "rubric": item["rubric"],
    })).collect::<Vec<_>>();
    assert!(
        view["pending_languages"]
            .as_array()
            .unwrap()
            .contains(&json!("en"))
    );
    edits[0]["name"] = json!("Water conservation");
    edits[0]["rubric"][0] = json!("Mentions reducing waste");
    let patch = |edits: &[Value]| {
        json_req(
            "PATCH",
            &path,
            Some(&actors.teacher),
            Some(json!({"language":"en", "dimensions": edits})),
        )
    };
    let updated = send(&app, patch(&edits)).await;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json_body(updated).await;
    assert_eq!(updated["dimensions"][0]["name"]["en"], "Water conservation");
    assert_eq!(
        updated["dimensions"][0]["name"]["zh_tw"],
        view["dimensions"][0]["name"]["zh_tw"]
    );
    assert_eq!(updated["pending_languages"], json!(["es"]));

    let student = seed_user(&pool, "dimension-lock@example.com").await;
    let conversation: Uuid = sqlx::query_scalar(
        "INSERT INTO conversations (user_id, topic_id, title) VALUES ($1, $2, 'Test') RETURNING id",
    )
    .bind(student)
    .bind(Uuid::parse_str(&id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM conversations WHERE id = $1")
        .bind(conversation)
        .execute(&pool)
        .await
        .unwrap();
    edits[0]["rubric"][0] = json!("A changed criterion");
    assert_eq!(
        send(&app, patch(&edits)).await.status(),
        StatusCode::CONFLICT
    );
    edits.pop();
    assert_eq!(
        send(&app, patch(&edits)).await.status(),
        StatusCode::BAD_REQUEST
    );
    edits = updated["dimensions"].as_array().unwrap().iter().map(|item| json!({
        "id": item["id"], "name": item["name"]["en"], "description": item["description"]["en"],
    })).collect();
    edits[0]["description"] = json!("Notice opportunities to conserve water.");
    let revised = send(&app, patch(&edits)).await;
    assert_eq!(revised.status(), StatusCode::OK);
    assert_eq!(json_body(revised).await["locked"], true);
    assert_eq!(ai.requests.lock().unwrap().len(), 3);
}

#[sqlx::test]
async fn only_teachers_and_administrators_can_manage_topic_dimensions(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let id = create(&app, &actors.teacher).await;
    let path = format!("/api/topics/{id}/dimensions");
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        for (method, path) in [
            ("GET", path.clone()),
            ("PATCH", path.clone()),
            ("POST", format!("{path}/retry")),
        ] {
            let request = json_req(
                method,
                &path,
                cookie,
                (method == "PATCH").then(|| json!({})),
            );
            assert_eq!(send(&app, request).await.status(), status);
        }
    }
    assert_eq!(
        send(&app, get(&path, Some(&actors.admin))).await.status(),
        StatusCode::OK
    );
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    assert!(doc["paths"]["/api/topics/{id}/dimensions"]["patch"]["requestBody"].is_object());
    assert!(doc["paths"]["/api/topics/{id}/dimensions"]["patch"]["responses"]["409"].is_object());
}
