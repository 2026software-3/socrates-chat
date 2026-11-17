//! F-29.4: activity topics remain separate, including topics without summaries.
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn dashboard_includes_each_topic_in_activity_order_even_without_summaries(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "第一題").await;
    let t2 = create_topic(&app, &a, "第二題").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "多題活動", "classroom_id": a.classroom, "topic_ids": [t2, t1]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let id = json_body(res).await["id"].clone();
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    let activity = d["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["activity_id"] == id)
        .unwrap();
    assert_eq!(activity["topics"][0]["topic_id"], t2);
    assert_eq!(activity["topics"][1]["topic_id"], t1);
    assert_eq!(activity["topics"][0]["completed"], 0);
    assert_eq!(activity["topics"][0]["claims"], json!([]));
}

#[sqlx::test]
async fn groups_regroup_and_deletion_are_scoped_to_one_activity_topic(pool: PgPool) {
    use common::Script::Reply;
    let ai = FakeAi::with(vec![
        Reply(vec![
            r#"{"stance":"s","reasons":"理由甲","claim":"甲主張"}"#,
        ]),
        Reply(vec![r#"{"group":"同名群"}"#]),
        Reply(vec![
            r#"{"stance":"s","reasons":"理由乙","claim":"乙主張"}"#,
        ]),
        Reply(vec![r#"{"group":"同名群"}"#]),
        Reply(vec![r#"{"groups":[{"name":"新甲群","members":[1]}]}"#]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    // Keep scripted AI replies for summaries and grouping, not topic dimension generation.
    let t1 = seed_topic("第一題").await;
    let t2 = seed_topic("第二題").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "多題活動", "classroom_id": a.classroom, "topic_ids": [t1, t2]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let act = json_body(res).await["id"].as_str().unwrap().to_owned();
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let mut conversations = Vec::new();
    for topic in [&t1, &t2] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&a.student),
                Some(json!({"activity_id": act,"topic_id":topic})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let id = json_body(res).await["id"].as_str().unwrap().to_owned();
        seed_student_message(&pool, &id, "秘密聊天原文").await;
        send(
            &app,
            json_req(
                "POST",
                &format!("/api/conversations/{id}/end"),
                Some(&a.student),
                None,
            ),
        )
        .await;
        let mut grouped = false;
        for _ in 0..200 {
            grouped = sqlx::query_scalar::<_, bool>(
                "SELECT claim_group_id IS NOT NULL FROM summaries WHERE conversation_id = $1",
            )
            .bind(id.parse::<uuid::Uuid>().unwrap())
            .fetch_optional(&pool)
            .await
            .unwrap()
            .unwrap_or(false);
            if grouped {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(grouped, "background grouping must be persisted");
        conversations.push(id);
    }
    assert!(
        !ai.requests.lock().unwrap()[3].system.contains("- 同名群"),
        "第二題不可沿用第一題群組"
    );
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            Some(&a.teacher),
            Some(json!({"activity_id":act,"topic_id":t1})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    let topics = &d["activities"][0]["topics"];
    assert_eq!(topics[0]["claims"][0]["name"], "新甲群");
    assert_eq!(topics[1]["claims"][0]["name"], "同名群");
    {
        let requests = ai.requests.lock().unwrap();
        let regroup = &requests[4];
        assert!(regroup.system.contains("第一題") && regroup.system.contains("甲主張"));
        assert!(!regroup.system.contains("乙主張") && !regroup.system.contains("秘密聊天原文"));
        assert!(!regroup.system.contains("@example.com"));
    }
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{}", conversations[0]),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    assert_eq!(d["activities"][0]["topics"][0]["completed"], 0);
    assert_eq!(d["activities"][0]["topics"][1]["completed"], 1);
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.admin))).await).await;
    assert_eq!(d["masked"], true);
    assert_eq!(d["activities"], json!([]));
}

#[sqlx::test(migrations = false)]
async fn migration_splits_existing_groups_without_losing_summaries_or_memberships(pool: PgPool) {
    use sqlx::migrate::Migrator;
    let all = sqlx::migrate!("./migrations");
    Migrator::with_migrations(
        all.iter()
            .filter(|m| m.version < 20261110000000)
            .cloned()
            .collect(),
    )
    .run(&pool)
    .await
    .unwrap();
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "甲題").await;
    let t2 = create_topic(&app, &a, "乙題").await;
    let act = publish_activity(&app, &a, "舊活動", &t1).await;
    let act_id: uuid::Uuid = act.parse().unwrap();
    sqlx::query("INSERT INTO activity_topics (activity_id, topic_id, position) VALUES ($1,$2,1)")
        .bind(act_id)
        .bind(t2.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let group: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO claim_groups (activity_id,name) VALUES ($1,'既有群') RETURNING id",
    )
    .bind(act_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    for topic in [&t1, &t2] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&a.student),
                Some(json!({"activity_id":act,"topic_id":topic})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let id: uuid::Uuid = json_body(res).await["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        sqlx::query("INSERT INTO summaries (conversation_id,status,stance,reasons,claim,claim_group_id,completed_at) VALUES ($1,'ready','立場','理由','主張',$2,now())")
            .bind(id).bind(group).execute(&pool).await.unwrap();
    }
    let legacy: uuid::Uuid = sqlx::query_scalar("INSERT INTO conversations (user_id, activity_id, classroom_id, title) SELECT user_id, activity_id, classroom_id, '歷史缺題目' FROM conversations WHERE activity_id=$1 LIMIT 1 RETURNING id")
        .bind(act_id).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO summaries (conversation_id,status,stance,reasons,claim,claim_group_id,completed_at) VALUES ($1,'ready','舊立場','舊理由','舊主張',$2,now())")
        .bind(legacy).bind(group).execute(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let rows: Vec<(uuid::Uuid,uuid::Uuid,String,String)> = sqlx::query_as("SELECT c.topic_id,g.topic_id,g.name,s.claim FROM summaries s JOIN conversations c ON c.id=s.conversation_id JOIN claim_groups g ON g.id=s.claim_group_id WHERE c.activity_id=$1 AND c.topic_id IS NOT NULL ORDER BY c.topic_id")
        .bind(act_id).fetch_all(&pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    for (conversation_topic, group_topic, name, claim) in rows {
        assert_eq!(conversation_topic, group_topic);
        assert_eq!(name, "既有群");
        assert_eq!(claim, "主張");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM claim_groups WHERE activity_id=$1")
        .bind(act_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
    let preserved: (Option<uuid::Uuid>, uuid::Uuid, String, String, String) = sqlx::query_as("SELECT g.topic_id,g.id,s.stance,s.reasons,s.claim FROM summaries s JOIN claim_groups g ON g.id=s.claim_group_id WHERE s.conversation_id=$1")
        .bind(legacy).fetch_one(&pool).await.unwrap();
    assert_eq!(
        preserved,
        (
            None,
            group,
            "舊立場".into(),
            "舊理由".into(),
            "舊主張".into()
        )
    );
    all.run(&pool).await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM claim_groups WHERE activity_id=$1")
        .bind(act_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, count);
}

#[sqlx::test]
async fn regroup_rejects_unrelated_topics_and_preserves_authorization(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "活動題目").await;
    let unrelated = create_topic(&app, &a, "其他題目").await;
    let activity = publish_activity(&app, &a, "活動", &topic).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.admin.as_str()), StatusCode::FORBIDDEN),
    ] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/dashboard/class/regroup",
                cookie,
                Some(json!({"activity_id":activity,"topic_id":topic})),
            ),
        )
        .await;
        assert_eq!(res.status(), status);
    }
    let mut errors = Vec::new();
    for topic in [unrelated, uuid::Uuid::new_v4().to_string()] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/dashboard/class/regroup",
                Some(&a.teacher),
                Some(json!({"activity_id":activity,"topic_id":topic})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let mut body = json_body(res).await;
        body["error"].as_object_mut().unwrap().remove("request_id");
        errors.push(body);
    }
    assert_eq!(errors[0], errors[1]);
}
