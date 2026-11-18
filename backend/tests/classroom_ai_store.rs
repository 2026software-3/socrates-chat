//! F-40.4-DB (#596): 教室的對話模型、備援模型、供應商確認與「過渡中」旗標；資料皆為合成。
mod common;

use common::*;
use socrates_chat_backend::classroom_ai_store::{self as store, ModelRef, SetError};
use sqlx::{PgPool, migrate::Migrator};
use std::borrow::Cow;
use uuid::Uuid;

fn m(provider: &str, model: &str) -> ModelRef {
    ModelRef {
        provider: provider.into(),
        model: model.into(),
    }
}

async fn classroom(pool: &PgPool, owner: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO classrooms (name, created_by) VALUES ($1, $2) RETURNING id")
        .bind(name)
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn new_classroom_has_no_setting_and_is_not_transitional(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let c = classroom(&pool, t, "c1").await;
    assert!(store::get(&pool, c).await.unwrap().is_none());
    assert!(!store::is_transitional(&pool, c).await.unwrap());
}

#[sqlx::test]
async fn set_saves_primary_and_ordered_fallbacks_and_replaces_them(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let c = classroom(&pool, t, "c1").await;
    let fb = vec![m("anthropic", "claude-x"), m("gemini", "gem-y")];
    store::set(&pool, c, m("openai", "gpt-a"), fb.clone())
        .await
        .unwrap();
    let s = store::get(&pool, c).await.unwrap().unwrap();
    assert_eq!(s.primary, m("openai", "gpt-a"));
    assert_eq!(s.fallbacks, fb);

    // 重新設定：主模型更新、備援整組取代（順序以傳入為準）
    store::set(&pool, c, m("openai", "gpt-b"), vec![m("gemini", "gem-y")])
        .await
        .unwrap();
    let s = store::get(&pool, c).await.unwrap().unwrap();
    assert_eq!(s.primary.model, "gpt-b");
    assert_eq!(s.fallbacks, vec![m("gemini", "gem-y")]);
    store::set(&pool, c, m("openai", "gpt-b"), vec![])
        .await
        .unwrap();
    assert!(
        store::get(&pool, c)
            .await
            .unwrap()
            .unwrap()
            .fallbacks
            .is_empty()
    );
}

#[sqlx::test]
async fn at_most_three_fallbacks_and_invalid_values_are_rejected(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let c = classroom(&pool, t, "c1").await;
    let four = vec![m("a", "1"), m("b", "2"), m("c", "3"), m("d", "4")];
    assert!(matches!(
        store::set(&pool, c, m("openai", "x"), four).await,
        Err(SetError::TooManyFallbacks)
    ));
    for bad in [
        m("", "x"),
        m("OpenAI", "x"),
        m("openai", ""),
        m("openai", "   "),
    ] {
        assert!(matches!(
            store::set(&pool, c, bad, vec![]).await,
            Err(SetError::Invalid)
        ));
    }
    assert!(matches!(
        store::set(&pool, c, m("openai", "x"), vec![m("openai", "")]).await,
        Err(SetError::Invalid)
    ));
    // 失敗不留下半套設定
    assert!(store::get(&pool, c).await.unwrap().is_none());
}

#[sqlx::test]
async fn database_guards_fallback_position_and_uniqueness(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let c = classroom(&pool, t, "c1").await;
    store::set(&pool, c, m("openai", "x"), vec![])
        .await
        .unwrap();
    for pos in [0, 4] {
        let r = sqlx::query("INSERT INTO classroom_ai_fallbacks(classroom_id,position,provider,model) VALUES($1,$2,'a','b')")
            .bind(c).bind(pos).execute(&pool).await;
        assert!(r.is_err(), "position {pos}");
    }
    sqlx::query("INSERT INTO classroom_ai_fallbacks(classroom_id,position,provider,model) VALUES($1,1,'a','b')")
        .bind(c).execute(&pool).await.unwrap();
    let dup = sqlx::query("INSERT INTO classroom_ai_fallbacks(classroom_id,position,provider,model) VALUES($1,1,'c','d')")
        .bind(c).execute(&pool).await;
    assert!(dup.is_err());
}

#[sqlx::test]
async fn provider_confirmation_is_recorded_once_per_classroom_and_provider(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let other = seed_user(&pool, "t2@example.com").await;
    let c = classroom(&pool, t, "c1").await;
    assert!(!store::is_confirmed(&pool, c, "openai").await.unwrap());
    assert!(store::confirm(&pool, c, "openai", t).await.unwrap());
    assert!(store::is_confirmed(&pool, c, "openai").await.unwrap());
    // 第二次不覆蓋第一次的確認人與時間
    assert!(!store::confirm(&pool, c, "openai", other).await.unwrap());
    let who: Uuid = sqlx::query_scalar("SELECT confirmed_by FROM classroom_ai_provider_confirmations WHERE classroom_id=$1 AND provider='openai'")
        .bind(c).fetch_one(&pool).await.unwrap();
    assert_eq!(who, t);
    // 換供應商要再確認一次
    assert!(!store::is_confirmed(&pool, c, "anthropic").await.unwrap());
    assert!(store::confirm(&pool, c, "anthropic", t).await.unwrap());
}

#[sqlx::test]
async fn clearing_transitional_only_affects_that_classroom(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let c1 = classroom(&pool, t, "c1").await;
    let c2 = classroom(&pool, t, "c2").await;
    sqlx::query("UPDATE classrooms SET ai_transitional = true")
        .execute(&pool)
        .await
        .unwrap();
    assert!(store::clear_transitional(&pool, c1).await.unwrap());
    assert!(!store::clear_transitional(&pool, c1).await.unwrap());
    assert!(!store::is_transitional(&pool, c1).await.unwrap());
    assert!(store::is_transitional(&pool, c2).await.unwrap());
}

#[sqlx::test]
async fn deleting_a_classroom_removes_setting_fallbacks_and_confirmations(pool: PgPool) {
    let t = seed_user(&pool, "t@example.com").await;
    let c = classroom(&pool, t, "c1").await;
    store::set(&pool, c, m("openai", "x"), vec![m("gemini", "y")])
        .await
        .unwrap();
    store::confirm(&pool, c, "openai", t).await.unwrap();
    sqlx::query("DELETE FROM classrooms WHERE id=$1")
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM classroom_ai_models)
              + (SELECT count(*) FROM classroom_ai_fallbacks)
              + (SELECT count(*) FROM classroom_ai_provider_confirmations)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = false)]
async fn classrooms_existing_before_the_migration_become_transitional_and_it_is_repeatable(
    pool: PgPool,
) {
    let all = sqlx::migrate!();
    let legacy = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|m| m.version < 20261118000100)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();
    let t = seed_user(&pool, "t@example.com").await;
    let old = classroom(&pool, t, "old").await;
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    assert!(store::is_transitional(&pool, old).await.unwrap());
    // 既有資料不變
    let name: String = sqlx::query_scalar("SELECT name FROM classrooms WHERE id=$1")
        .bind(old)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "old");
    // migration 後新建立的不是過渡中
    let new = classroom(&pool, t, "new").await;
    assert!(!store::is_transitional(&pool, new).await.unwrap());
}
