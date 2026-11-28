//! #635: synthetic discussions; legacy topics retain their original rules.
mod common;

use axum::http::StatusCode;
use common::{Script::Reply, *};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn enable(pool: &PgPool, conversation: &str) {
    let dimensions: Vec<_> = (1..=6).map(|n| json!({
        "id": format!("d{n}"),
        "name": {"zh_tw": format!("立場{n}"), "en": format!("Position {n}"), "es": format!("Postura {n}")},
        "description": {"zh_tw":"贊成或反對", "en":"Agree or disagree", "es":"Acuerdo o desacuerdo"},
        "rubric": ["強烈反對", "稍微反對", "無感", "稍微贊成", "強烈贊成"]
    })).collect();
    sqlx::query("UPDATE topics SET radar_scale='stance', dimensions=$2, dimensions_prompt_version='topic-stance-dimensions-v1', dimensions_language='zh-TW' WHERE id=(SELECT topic_id FROM conversations WHERE id=$1)")
        .bind(conversation.parse::<Uuid>().unwrap()).bind(sqlx::types::Json(dimensions)).execute(pool).await.unwrap();
    sqlx::query("UPDATE conversations SET dimension_rules=true WHERE id=$1")
        .bind(conversation.parse::<Uuid>().unwrap())
        .execute(pool)
        .await
        .unwrap();
}

async fn say(app: &axum::Router, actors: &Actors, conversation: &str) -> Vec<(String, Value)> {
    let response = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/messages"),
            Some(&actors.student),
            Some(json!({"content":"我認為如此，因為這是我的理由。"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    parse_sse(&text_body(response).await)
}

#[sqlx::test]
async fn new_rules_neither_force_closure_at_old_limit_nor_trust_unasked_dimensions(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![
        r#"為什麼？<<<META>>>{"question_type":"reason","advance":false,"answered_dimensions":["d1","d2","d3","d4","d5","d6"],"question_dimensions":["d1"],"response_kind":"answered"}"#,
    ])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    enable(&pool, &conversation).await;
    sqlx::query("UPDATE conversations SET turn_count=25 WHERE id=$1")
        .bind(conversation.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let events = say(&app, &actors, &conversation).await;
    assert_eq!(events[0].1["auto_ended"], false);
    let (status, covered): (String, Vec<String>) =
        sqlx::query_as("SELECT status, covered_dimensions FROM conversations WHERE id=$1")
            .bind(conversation.parse::<Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "active");
    assert!(covered.is_empty());
    let request = ai.requests.lock().unwrap()[0].clone();
    assert!(request.system.contains("d6"));
    assert!(!request.system.contains("第 15 回合"));
    assert_eq!(
        request.messages.last().unwrap().content,
        "我認為如此，因為這是我的理由。"
    );
}

#[sqlx::test]
async fn offense_and_off_topic_do_not_complete_a_dimension_but_indifference_does(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![
            r#"你怎麼看？<<<META>>>{"question_type":"reason","question_dimensions":["d1"]}"#,
        ]),
        Reply(vec![
            r#"換個情境，你怎麼看？<<<META>>>{"question_type":"reason","answered_dimensions":["d1"],"question_dimensions":["d1"],"response_kind":"offended"}"#,
        ]),
        Reply(vec![
            r#"回到這個情境，你怎麼看？<<<META>>>{"question_type":"reason","answered_dimensions":["d1"],"question_dimensions":["d1"],"response_kind":"off_topic"}"#,
        ]),
        Reply(vec![
            r#"那另一個角度呢？<<<META>>>{"question_type":"reason","answered_dimensions":["d1"],"question_dimensions":["d2"],"response_kind":"indifferent"}"#,
        ]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai);
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    enable(&pool, &conversation).await;
    for expected in [vec![], vec![], vec![], vec!["d1".to_owned()]] {
        say(&app, &actors, &conversation).await;
        let covered: Vec<String> =
            sqlx::query_scalar("SELECT covered_dimensions FROM conversations WHERE id=$1")
                .bind(conversation.parse::<Uuid>().unwrap())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(covered, expected);
    }
}

#[sqlx::test]
async fn convergence_waits_for_all_six_responses_and_then_ends(pool: PgPool) {
    let replies: Vec<&'static str> = (0..=6)
        .map(|n| {
            let meta = json!({
                "question_type": if n == 6 { "wrap_up" } else { "perspective" },
                "advance": true,
                "answered_dimensions": if n == 0 { vec![] } else { vec![format!("d{n}")] },
                "question_dimensions": if n == 6 { vec![] } else { vec![format!("d{}", n+1)] },
                "response_kind":"answered"
            });
            &*Box::leak(format!("我們看看這個角度。<<<META>>>{meta}").into_boxed_str())
        })
        .collect();
    let ai = FakeAi::with(replies.into_iter().map(|s| Reply(vec![s])).collect());
    let app = test_app_with_ai(pool.clone(), ai);
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    enable(&pool, &conversation).await;
    sqlx::query("UPDATE conversations SET stage=3 WHERE id=$1")
        .bind(conversation.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    for n in 0..=6 {
        let events = say(&app, &actors, &conversation).await;
        let done = events.iter().find(|(event, _)| event == "done").unwrap();
        assert_eq!(done.1["ended"], n == 6);
    }
}

#[test]
fn signed_partial_scores_distinguish_unknown_from_indifference_and_preserve_legacy() {
    use socrates_chat_backend::{
        radar_scoring::{Rejected, parse_scores},
        radar_store::Rubric,
    };
    let rubric = Rubric::stance((1..=6).map(|n| format!("d{n}")).collect());
    let scores = parse_scores(
        r#"{"d1":-2,"d2":-1,"d3":0,"d4":1,"d5":2,"d6":null}"#,
        &rubric,
    )
    .unwrap();
    assert_eq!(scores.len(), 5);
    assert_eq!(scores["d1"], -2);
    assert_eq!(scores["d3"], 0);
    assert!(!scores.contains_key("d6"));
    assert!(parse_scores("{}", &rubric).unwrap().is_empty());
    for raw in [
        r#"{"d1":3}"#,
        r#"{"d1":-3}"#,
        r#"{"d1":2.0}"#,
        r#"{"d1":true}"#,
        r#"{"d7":null}"#,
        r#"{"d1":null,"d1":0}"#,
    ] {
        assert!(parse_scores(raw, &rubric).is_err(), "{raw}");
    }
    assert_eq!(
        parse_scores(
            r#"{"d1":-1,"d2":0,"d3":0,"d4":0,"d5":0,"d6":0}"#,
            &Rubric::standard()
        ),
        Err(Rejected::NotAnInteger)
    );
}

#[sqlx::test]
async fn manual_end_preserves_summary_and_returns_signed_partial_radar(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![r#"{"stance":"合成立場","reasons":"合成理由"}"#]),
        Reply(vec![
            r#"{"d1":-2,"d2":0,"d3":1,"d4":null,"d5":2,"d6":null}"#,
        ]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    enable(&pool, &conversation).await;
    seed_student_message(&pool, &conversation, "我反對第一點，但不在乎第二點。").await;
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/end"),
            Some(&actors.student),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &actors.student, &conversation).await["status"],
        "ready"
    );
    let path = format!("/api/conversations/{conversation}/radar");
    let mut result = Value::Null;
    for _ in 0..200 {
        result = json_body(send(&app, get(&path, Some(&actors.student))).await).await;
        if result["status"] == "ready" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(result["status"], "ready", "{result}");
    assert_eq!(result["scale"], "stance");
    assert_eq!(result["scores"], json!({"d1":-2,"d2":0,"d3":1,"d5":2}));
    assert_eq!(result["rules_version"], "radar-stance-v1");
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(&actors.teacher), StatusCode::FORBIDDEN),
        (Some(&actors.admin), StatusCode::FORBIDDEN),
        (Some(&actors.outsider), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(&app, get(&path, cookie.map(String::as_str)))
                .await
                .status(),
            status
        );
    }
    let request = &ai.requests.lock().unwrap()[1];
    assert!(request.system.contains("Lack of evidence is NOT 0"));
    let input: Value = serde_json::from_str(&request.messages[0].content).unwrap();
    assert!(input["dimensions"][0]["rubric"].get("-2").is_some());
    assert!(input["dimensions"][0]["rubric"].get("5").is_none());
}

#[sqlx::test]
async fn missing_topic_dimensions_waive_coverage_but_coverage_alone_does_not_end(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![
            r#"再看看？<<<META>>>{"question_type":"wrap_up","advance":false}"#,
        ]),
        Reply(vec![
            r#"討論結束。<<<META>>>{"question_type":"wrap_up","advance":true}"#,
        ]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai);
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    enable(&pool, &conversation).await;
    sqlx::query("UPDATE conversations SET stage=3, covered_dimensions=ARRAY['d1','d2','d3','d4','d5','d6'] WHERE id=$1")
        .bind(conversation.parse::<Uuid>().unwrap()).execute(&pool).await.unwrap();
    let events = say(&app, &actors, &conversation).await;
    assert_eq!(events.last().unwrap().1["ended"], false);
    sqlx::query("UPDATE topics SET dimensions=NULL, dimensions_prompt_version=NULL, dimensions_language=NULL WHERE id=(SELECT topic_id FROM conversations WHERE id=$1)")
        .bind(conversation.parse::<Uuid>().unwrap()).execute(&pool).await.unwrap();
    let events = say(&app, &actors, &conversation).await;
    assert_eq!(events.last().unwrap().1["ended"], true);
}

#[sqlx::test]
async fn only_new_topic_creation_enables_the_new_rules(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let old = start_conversation(&app, &actors).await;
    let id = create_topic(&app, &actors, "新的合成立場題目").await;
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&actors.student),
            Some(json!({"topic_id":id})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let new = json_body(response).await["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let new_rules: bool =
        sqlx::query_scalar("SELECT dimension_rules FROM conversations WHERE id=$1")
            .bind(new)
            .fetch_one(&pool)
            .await
            .unwrap();
    let old_rules: bool =
        sqlx::query_scalar("SELECT dimension_rules FROM conversations WHERE id=$1")
            .bind(old.parse::<Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(new_rules);
    assert!(!old_rules);
}

#[sqlx::test]
async fn hearing_only_the_intro_does_not_count_as_hearing_the_dimension_question(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![
            r#"我明白你的理由，那你贊成公開理由嗎？<<<META>>>{"question_type":"perspective","question_dimensions":["d1"]}"#,
        ]),
        Reply(vec![
            r#"你怎麼看？<<<META>>>{"question_type":"perspective","question_dimensions":["d1"],"answered_dimensions":["d1"],"response_kind":"answered"}"#,
        ]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai);
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    enable(&pool, &conversation).await;
    say(&app, &actors, &conversation).await;
    sqlx::query("UPDATE messages SET heard_chars=1 WHERE conversation_id=$1 AND role='ai'")
        .bind(conversation.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    say(&app, &actors, &conversation).await;
    let covered: Vec<String> =
        sqlx::query_scalar("SELECT covered_dimensions FROM conversations WHERE id=$1")
            .bind(conversation.parse::<Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(covered.is_empty());
}

#[sqlx::test(migrations = false)]
async fn forward_migration_retains_existing_results_and_rule_versions(pool: PgPool) {
    use sqlx::migrate::Migrator;
    let all = sqlx::migrate!();
    let old = Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|m| m.version < 20261125000000)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    old.run(&pool).await.unwrap();
    let app = test_app(pool.clone());
    let _actors = seed_actors(&app).await;
    let topic: Uuid =
        sqlx::query_scalar("INSERT INTO topics(title) VALUES('合成舊題目') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let conversation: Uuid = sqlx::query_scalar("INSERT INTO conversations(user_id,topic_id,title,status,ended_at) SELECT id,$1,'合成舊對話','ended',now() FROM users WHERE email='student@example.com' RETURNING id")
        .bind(topic).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO summaries(conversation_id,status,stance,reasons,completed_at) VALUES($1,'ready','舊立場','舊理由',now())").bind(conversation).execute(&pool).await.unwrap();
    let scores = json!({"d1":5,"d2":0,"d3":1,"d4":2,"d5":3,"d6":4});
    sqlx::query("INSERT INTO conversation_radar_results(conversation_id,status,scores,rules_version,completed_at) VALUES($1,'ready',$2,'radar-scoring-v1',now())")
        .bind(conversation).bind(&scores).execute(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let (retained, version, rules, scale): (Value, String, bool, String) = sqlx::query_as("SELECT r.scores,r.rules_version,c.dimension_rules,t.radar_scale FROM conversation_radar_results r JOIN conversations c ON c.id=r.conversation_id JOIN topics t ON t.id=c.topic_id WHERE c.id=$1")
        .bind(conversation).fetch_one(&pool).await.unwrap();
    assert_eq!(retained, scores);
    assert_eq!(version, "radar-scoring-v1");
    assert!(!rules);
    assert_eq!(scale, "legacy");
}
