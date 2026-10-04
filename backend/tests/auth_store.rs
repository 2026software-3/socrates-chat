//! F-01.1-DB 資料存取層整合測試。使用 `sqlx::test`（每個測試一個獨立資料庫）與合成資料。
//! 需要 `DATABASE_URL` 指向可建立資料庫的 PostgreSQL（見 docker-compose.yml）。

use chrono::{Duration, Utc};
use socrates_chat_backend::auth_store::{
    create_session, delete_session, find_active_session, save_login_state, take_login_state,
    upsert_user,
};
use sqlx::PgPool;
use uuid::Uuid;

const HASH_A: [u8; 32] = [1; 32];
const HASH_B: [u8; 32] = [2; 32];

/// 測試用的一次性值（避免把固定值當作 nonce 寫死）。
fn random_value() -> String {
    format!("test-{}", Uuid::new_v4())
}

#[sqlx::test]
async fn upsert_creates_user_with_lowercased_email(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "Alice@Example.COM", Some("Alice"), false)
        .await
        .unwrap();
    assert_eq!(u.google_sub.as_deref(), Some("sub-1"));
    assert_eq!(u.email, "alice@example.com");
    assert_eq!(u.display_name.as_deref(), Some("Alice"));
    assert!(!u.is_admin);
}

#[sqlx::test]
async fn second_login_reuses_user_and_updates_email(pool: PgPool) {
    let first = upsert_user(&pool, "sub-1", "old@example.com", Some("Alice"), false)
        .await
        .unwrap();
    let second = upsert_user(&pool, "sub-1", "New@example.com", Some("Alice"), false)
        .await
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(second.email, "new@example.com");
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn admin_is_granted_only_on_first_login(pool: PgPool) {
    let admin = upsert_user(&pool, "sub-a", "a@example.com", None, true)
        .await
        .unwrap();
    assert!(admin.is_admin);
    // 之後不再依環境變數判斷：已是管理者的不會被降級
    let again = upsert_user(&pool, "sub-a", "a@example.com", None, false)
        .await
        .unwrap();
    assert!(again.is_admin);
    // 既有的非管理者不會因為之後出現在 ADMIN_EMAILS 而升級
    upsert_user(&pool, "sub-b", "b@example.com", None, false)
        .await
        .unwrap();
    let b = upsert_user(&pool, "sub-b", "b@example.com", None, true)
        .await
        .unwrap();
    assert!(!b.is_admin);
}

#[sqlx::test]
async fn session_lookup_returns_user_and_touches_last_seen(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    create_session(&pool, u.id, &HASH_A).await.unwrap();
    let now = Utc::now() + Duration::days(1);
    let found = find_active_session(&pool, &HASH_A, now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.user.id, u.id);
    assert_eq!(found.session.user_id, u.id);
    let again = find_active_session(&pool, &HASH_A, now)
        .await
        .unwrap()
        .unwrap();
    assert!(again.session.last_seen_at >= now - Duration::seconds(1));
}

#[sqlx::test]
async fn unknown_token_finds_nothing(pool: PgPool) {
    assert!(
        find_active_session(&pool, &HASH_B, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn idle_session_expires_after_7_days(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    create_session(&pool, u.id, &HASH_A).await.unwrap();
    let now = Utc::now() + Duration::days(7) + Duration::minutes(1);
    assert!(
        find_active_session(&pool, &HASH_A, now)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn session_expires_at_absolute_30_days_even_if_active(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    create_session(&pool, u.id, &HASH_A).await.unwrap();
    // 每 5 天使用一次，維持未閒置
    let start = Utc::now();
    for d in [5, 10, 15, 20, 25] {
        let now = start + Duration::days(d);
        assert!(
            find_active_session(&pool, &HASH_A, now)
                .await
                .unwrap()
                .is_some()
        );
    }
    let now = start + Duration::days(30) + Duration::minutes(1);
    assert!(
        find_active_session(&pool, &HASH_A, now)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn deleted_session_is_gone(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    create_session(&pool, u.id, &HASH_A).await.unwrap();
    assert!(delete_session(&pool, &HASH_A).await.unwrap());
    assert!(
        find_active_session(&pool, &HASH_A, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
    assert!(!delete_session(&pool, &HASH_A).await.unwrap());
}

#[sqlx::test]
async fn duplicate_token_hash_is_rejected(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    create_session(&pool, u.id, &HASH_A).await.unwrap();
    assert!(create_session(&pool, u.id, &HASH_A).await.is_err());
}

#[sqlx::test]
async fn each_login_creates_a_new_session(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    let s1 = create_session(&pool, u.id, &HASH_A).await.unwrap();
    let s2 = create_session(&pool, u.id, &HASH_B).await.unwrap();
    assert_ne!(s1.id, s2.id);
}

#[sqlx::test]
async fn login_state_is_single_use(pool: PgPool) {
    let nonce = random_value();
    save_login_state(&pool, "st", &nonce, "verifier")
        .await
        .unwrap();
    let got = take_login_state(&pool, "st", Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.nonce, nonce);
    assert_eq!(got.pkce_verifier, "verifier");
    assert!(
        take_login_state(&pool, "st", Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn unknown_login_state_is_rejected(pool: PgPool) {
    assert!(
        take_login_state(&pool, "nope", Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn expired_login_state_is_rejected_and_consumed(pool: PgPool) {
    save_login_state(&pool, "st", &random_value(), "v")
        .await
        .unwrap();
    let late = Utc::now() + Duration::minutes(11);
    assert!(take_login_state(&pool, "st", late).await.unwrap().is_none());
    assert!(
        take_login_state(&pool, "st", Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn deleting_user_cascades_to_sessions(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", None, false)
        .await
        .unwrap();
    create_session(&pool, u.id, &HASH_A).await.unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(u.id)
        .execute(&pool)
        .await
        .unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn migrations_are_rerunnable(pool: PgPool) {
    // sqlx::test 已套用一次；再執行應為 no-op
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
}
