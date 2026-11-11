//! F-24.1-DB: 單場六維分數保存；所有資料皆為合成。
mod common;

use common::*;
use serde_json::json;
use socrates_chat_backend::radar_store::{self, Scores};
use sqlx::{PgPool, migrate::Migrator};
use std::borrow::Cow;
use uuid::Uuid;

async fn ready_conversation(pool: &PgPool) -> Uuid {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = start_conversation(&app, &a).await.parse::<Uuid>().unwrap();
    sqlx::query("UPDATE conversations SET status='ended', ended_at=now() WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO summaries(conversation_id,status,stance,reasons,completed_at) VALUES($1,'ready','synthetic stance','synthetic reasons',now())")
        .bind(id).execute(pool).await.unwrap();
    id
}

#[sqlx::test]
async fn database_rejects_partial_and_invalid_scores_without_touching_summary(pool: PgPool) {
    let id = ready_conversation(&pool).await;
    for scores in [
        json!([0, 1, 2, 3, 4]),
        json!([0, 1, 2, 3, 4, 6]),
        json!([0, 1, 2, 3, 4, -1]),
        json!([0, 1, 2, 3, 4, null]),
        json!([0, 1, 2, 3, 4, 1.5]),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":6}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":-1}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":null}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":1.5}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"other":5}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":true}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":"5"}),
        json!({"d1":0,"d2":1,"d3":2,"d4":3,"d5":4,"d6":5,"d7":2}),
    ] {
        let result = sqlx::query("INSERT INTO conversation_radar_results(conversation_id,status,scores,rules_version,completed_at) VALUES($1,'ready',$2,'test/v1',now())")
            .bind(id).bind(scores).execute(&pool).await;
        let error = result.expect_err("invalid score sets must be rejected by the database");
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    let status: String =
        sqlx::query_scalar("SELECT status FROM summaries WHERE conversation_id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "ready");
}

fn scores() -> Scores {
    (1..=6).map(|d| (format!("d{d}"), (d - 1) as u8)).collect()
}

#[sqlx::test]
async fn retry_keeps_summary_and_old_generations_cannot_overwrite_results(pool: PgPool) {
    let id = ready_conversation(&pool).await;
    assert!(radar_store::get(&pool, id).await.unwrap().is_none());
    let first = radar_store::claim(&pool, id).await.unwrap().unwrap();
    assert_eq!(first, 1);
    assert!(radar_store::claim(&pool, id).await.unwrap().is_none());
    assert!(radar_store::fail(&pool, id, first).await.unwrap());
    let failed = radar_store::get(&pool, id).await.unwrap().unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.scores.is_none());
    let second = radar_store::claim(&pool, id).await.unwrap().unwrap();
    assert_eq!(second, 2);
    assert!(
        !radar_store::complete(&pool, id, first, &scores(), "test/v1")
            .await
            .unwrap()
    );
    assert!(!radar_store::fail(&pool, id, first).await.unwrap());
    assert!(
        radar_store::complete(&pool, id, second, &scores(), "test/v2")
            .await
            .unwrap()
    );
    let ready = radar_store::get(&pool, id).await.unwrap().unwrap();
    assert_eq!(ready.status, "ready");
    assert_eq!(ready.scores.unwrap().0, scores());
    assert_eq!(ready.rules_version.as_deref(), Some("test/v2"));
    assert!(ready.completed_at.is_some());
    assert!(radar_store::claim(&pool, id).await.unwrap().is_none());
    assert!(!radar_store::fail(&pool, id, second).await.unwrap());
    let summary: (String, String) =
        sqlx::query_as("SELECT status,stance FROM summaries WHERE conversation_id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(summary, ("ready".into(), "synthetic stance".into()));
}

#[sqlx::test]
async fn invalid_completion_leaves_no_partial_scores_and_can_be_marked_failed(pool: PgPool) {
    let id = ready_conversation(&pool).await;
    let attempt = radar_store::claim(&pool, id).await.unwrap().unwrap();
    let mut incomplete = scores();
    incomplete.remove("d6");
    for (value, version) in [(&incomplete, "test/v1"), (&scores(), " \t")] {
        let error = radar_store::complete(&pool, id, attempt, value, version)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        let result = radar_store::get(&pool, id).await.unwrap().unwrap();
        assert_eq!(result.status, "pending");
        assert!(result.scores.is_none());
        assert!(result.rules_version.is_none());
    }
    assert!(radar_store::fail(&pool, id, attempt).await.unwrap());
}

#[sqlx::test]
async fn concurrent_claims_only_start_one_attempt_and_startup_recovery_allows_retry(pool: PgPool) {
    let id = ready_conversation(&pool).await;
    let (one, two) = tokio::join!(radar_store::claim(&pool, id), radar_store::claim(&pool, id));
    let claimed: Vec<i32> = [one.unwrap(), two.unwrap()].into_iter().flatten().collect();
    assert_eq!(claimed, [1]);
    assert_eq!(radar_store::recover_orphaned(&pool).await.unwrap(), 1);
    assert_eq!(radar_store::recover_orphaned(&pool).await.unwrap(), 0);
    assert_eq!(radar_store::claim(&pool, id).await.unwrap(), Some(2));
}

#[sqlx::test]
async fn results_are_isolated_by_conversation_and_cascade_on_deletion(pool: PgPool) {
    let first = ready_conversation(&pool).await;
    let second: Uuid = sqlx::query_scalar("INSERT INTO conversations(user_id,topic_id,title,description,status,ended_at) SELECT user_id,topic_id,'other conversation','synthetic','ended',now() FROM conversations WHERE id=$1 RETURNING id")
        .bind(first).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO summaries(conversation_id,status,stance,reasons,completed_at) VALUES($1,'ready','other stance','other reasons',now())")
        .bind(second).execute(&pool).await.unwrap();
    let other_scores: Scores = scores()
        .into_iter()
        .map(|(key, score)| (key, 5 - score))
        .collect();
    for (id, value) in [(first, scores()), (second, other_scores.clone())] {
        let attempt = radar_store::claim(&pool, id).await.unwrap().unwrap();
        assert!(
            radar_store::complete(&pool, id, attempt, &value, "test/v1")
                .await
                .unwrap()
        );
    }
    sqlx::query("DELETE FROM conversations WHERE id=$1")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    assert!(radar_store::get(&pool, first).await.unwrap().is_none());
    assert_eq!(
        radar_store::get(&pool, second)
            .await
            .unwrap()
            .unwrap()
            .scores
            .unwrap()
            .0,
        other_scores
    );
}

#[sqlx::test]
async fn calculation_requires_an_ended_conversation_and_successful_summary(pool: PgPool) {
    let id = ready_conversation(&pool).await;
    for (conversation_status, summary_status) in [
        ("active", "ready"),
        ("ended", "pending"),
        ("ended", "failed"),
    ] {
        sqlx::query("UPDATE conversations SET status=$2 WHERE id=$1")
            .bind(id)
            .bind(conversation_status)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE summaries SET status=$2, completed_at=CASE WHEN $2='pending' THEN NULL ELSE now() END WHERE conversation_id=$1").bind(id).bind(summary_status).execute(&pool).await.unwrap();
        assert!(radar_store::claim(&pool, id).await.unwrap().is_none());
        assert!(radar_store::get(&pool, id).await.unwrap().is_none());
    }
    sqlx::query("DELETE FROM summaries WHERE conversation_id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(radar_store::claim(&pool, id).await.unwrap().is_none());
    assert!(
        radar_store::claim(&pool, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrations = false)]
async fn forward_migration_preserves_existing_summary_and_is_repeatable(pool: PgPool) {
    let all = sqlx::migrate!();
    let legacy = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|m| m.version < 20261112000000)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();
    let id = ready_conversation(&pool).await;
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let summary: (String, String, String) =
        sqlx::query_as("SELECT status,stance,reasons FROM summaries WHERE conversation_id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        summary,
        (
            "ready".into(),
            "synthetic stance".into(),
            "synthetic reasons".into()
        )
    );
    let exists: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('conversation_radar_results')::text")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(exists.as_deref(), Some("conversation_radar_results"));
}
