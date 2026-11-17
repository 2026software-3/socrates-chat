//! F-40.1-DB (#591): 教師 API key 的加密保存與資料存取；所有 key 皆為合成字串。
mod common;

use common::*;
use socrates_chat_backend::teacher_key_crypto::{KeyCipher, fingerprint, mask};
use socrates_chat_backend::teacher_key_store::{self as store, AddError, KeyStatus};
use sqlx::{PgPool, migrate::Migrator};
use std::borrow::Cow;

const KEY_A: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const KEY_B: &str = "sk-synthetic-BBBB-1111-2222-3333-bbbb";
// 32 個位元組的 base64（合成）
const ENC_1: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const ENC_2: &str = "k2:ZmVkY2JhOTg3NjU0MzIxMGZlZGNiYTk4NzY1NDMyMTA=";

fn cipher() -> KeyCipher {
    KeyCipher::parse(ENC_1).unwrap()
}

#[test]
fn cipher_round_trips_and_ciphertext_hides_the_key() {
    let c = cipher();
    let sealed = c.encrypt(KEY_A);
    assert_eq!(sealed.key_id, "k1");
    assert!(
        !sealed
            .ciphertext
            .windows(KEY_A.len())
            .any(|w| w == KEY_A.as_bytes())
    );
    assert_eq!(
        c.decrypt(&sealed.key_id, &sealed.ciphertext).unwrap(),
        KEY_A
    );
    // 每次加密用新的 nonce
    assert_ne!(sealed.ciphertext, c.encrypt(KEY_A).ciphertext);
}

#[test]
fn cipher_rejects_tampering_unknown_key_id_and_wrong_key() {
    let c = cipher();
    let sealed = c.encrypt(KEY_A);
    let mut bad = sealed.ciphertext.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(c.decrypt("k1", &bad).is_err());
    assert!(c.decrypt("nope", &sealed.ciphertext).is_err());
    assert!(c.decrypt("k1", &[1, 2, 3]).is_err());
    let other = KeyCipher::parse(ENC_2).unwrap();
    assert!(other.decrypt("k2", &sealed.ciphertext).is_err());
}

#[test]
fn rotation_encrypts_with_first_key_and_still_decrypts_old_ones() {
    let old = cipher().encrypt(KEY_A);
    let rotated = KeyCipher::parse(&format!("{ENC_2},{ENC_1}")).unwrap();
    assert_eq!(rotated.encrypt(KEY_B).key_id, "k2");
    assert_eq!(
        rotated.decrypt(&old.key_id, &old.ciphertext).unwrap(),
        KEY_A
    );
}

#[test]
fn cipher_config_must_be_wellformed_and_never_echoes_secrets() {
    for bad in [
        "",
        "k1",
        "k1:",
        "k1:not-base64!!",
        "k1:c2hvcnQ=",
        "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=,k1:ZmVkY2JhOTg3NjU0MzIxMGZlZGNiYTk4NzY1NDMyMTA=",
    ] {
        let err = KeyCipher::parse(bad).err().unwrap();
        assert!(!err.to_string().contains("MDEyMzQ1"), "{err}");
    }
    assert!(!format!("{:?}", cipher()).contains("MDEyMzQ1"));
}

#[test]
fn fingerprint_is_stable_and_mask_shows_only_the_tail() {
    assert_eq!(fingerprint(KEY_A), fingerprint(KEY_A));
    assert_ne!(fingerprint(KEY_A), fingerprint(KEY_B));
    assert_eq!(fingerprint(KEY_A).len(), 32);
    assert_eq!(mask(KEY_A), "…aaaa");
    // 太短的 key 不露出任何字元
    assert_eq!(mask("short"), "…");
}

async fn teacher(pool: &PgPool, email: &str) -> uuid::Uuid {
    seed_user(pool, email).await
}

#[sqlx::test]
async fn add_stores_only_ciphertext_mask_and_fingerprint(pool: PgPool) {
    let t = teacher(&pool, "t1@example.com").await;
    let k = store::add(&pool, &cipher(), t, "openai", KEY_A)
        .await
        .unwrap();
    assert_eq!(k.provider, "openai");
    assert_eq!(k.mask, "…aaaa");
    assert_eq!(k.status, KeyStatus::Ok);
    // 直接查表：任何欄位都不含明文
    let row: (Vec<u8>, String, Vec<u8>, String) = sqlx::query_as(
        "SELECT ciphertext, mask, fingerprint, encryption_key_id FROM teacher_api_keys WHERE id=$1",
    )
    .bind(k.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !row.0
            .windows(8)
            .any(|w| KEY_A.as_bytes().windows(8).any(|p| p == w))
    );
    assert_eq!(row.1, "…aaaa");
    assert_eq!(row.2, fingerprint(KEY_A));
    assert_eq!(row.3, "k1");
    let as_text: String = sqlx::query_scalar("SELECT t::text FROM teacher_api_keys t")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!as_text.contains(KEY_A));
}

#[sqlx::test]
async fn duplicate_key_is_rejected_even_across_teachers_and_providers(pool: PgPool) {
    let t1 = teacher(&pool, "t1@example.com").await;
    let t2 = teacher(&pool, "t2@example.com").await;
    let c = cipher();
    store::add(&pool, &c, t1, "openai", KEY_A).await.unwrap();
    for (owner, provider) in [(t1, "openai"), (t2, "openai"), (t2, "anthropic")] {
        let err = store::add(&pool, &c, owner, provider, KEY_A)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, AddError::Duplicate), "{err:?}");
    }
    assert_eq!(store::list_for_owner(&pool, t2).await.unwrap().len(), 0);
    store::add(&pool, &c, t2, "openai", KEY_B).await.unwrap();
}

#[sqlx::test]
async fn replace_keeps_id_and_provider_and_rejects_duplicates(pool: PgPool) {
    let t1 = teacher(&pool, "t1@example.com").await;
    let t2 = teacher(&pool, "t2@example.com").await;
    let c = cipher();
    let a = store::add(&pool, &c, t1, "openai", KEY_A).await.unwrap();
    let other = store::add(&pool, &c, t2, "openai", KEY_B).await.unwrap();
    store::set_status(&pool, a.id, KeyStatus::Invalid)
        .await
        .unwrap();

    let new_key = "sk-synthetic-CCCC-1111-2222-3333-cccc";
    let r = store::replace(&pool, &c, t1, a.id, new_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((r.id, r.provider.as_str()), (a.id, "openai"));
    assert_eq!(r.mask, "…cccc");
    assert_eq!(r.created_at, a.created_at);
    // 換了新 key，舊的失效狀態清除
    assert_eq!(r.status, KeyStatus::Ok);
    let usable = store::keys_for(&pool, &c, t1, "openai").await.unwrap();
    assert_eq!(usable.len(), 1);
    assert_eq!(usable[0].secret(), new_key);
    // 舊 key 現在可以被別人綁定
    store::add(&pool, &c, t2, "openai", KEY_A).await.unwrap();

    // 換成別人的 key 被拒；換成自己現有的 key 視為沒變、不報重複
    let err = store::replace(&pool, &c, t1, a.id, KEY_B)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, AddError::Duplicate));
    assert!(
        store::replace(&pool, &c, t1, a.id, new_key)
            .await
            .unwrap()
            .is_some()
    );
    // 不是自己的、或不存在的 id
    assert!(
        store::replace(
            &pool,
            &c,
            t1,
            other.id,
            "sk-synthetic-DDDD-1111-2222-3333-dddd"
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        store::replace(
            &pool,
            &c,
            t1,
            uuid::Uuid::new_v4(),
            "sk-synthetic-EEEE-1111-2222-3333-eeee"
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[sqlx::test]
async fn list_is_scoped_to_the_owner_and_never_exposes_secrets(pool: PgPool) {
    let t1 = teacher(&pool, "t1@example.com").await;
    let t2 = teacher(&pool, "t2@example.com").await;
    let c = cipher();
    store::add(&pool, &c, t1, "openai", KEY_A).await.unwrap();
    store::add(&pool, &c, t2, "openai", KEY_B).await.unwrap();
    let mine = store::list_for_owner(&pool, t1).await.unwrap();
    assert_eq!(mine.len(), 1);
    let dump = format!("{mine:?}");
    assert!(!dump.contains(KEY_A) && !dump.contains(KEY_B));
}

#[sqlx::test]
async fn delete_is_owner_scoped_and_admin_delete_reports_the_owner(pool: PgPool) {
    let t1 = teacher(&pool, "t1@example.com").await;
    let t2 = teacher(&pool, "t2@example.com").await;
    let c = cipher();
    let a = store::add(&pool, &c, t1, "openai", KEY_A).await.unwrap();
    let b = store::add(&pool, &c, t2, "openai", KEY_B).await.unwrap();
    assert!(!store::delete(&pool, t1, b.id).await.unwrap());
    assert!(store::delete(&pool, t1, a.id).await.unwrap());
    assert!(!store::delete(&pool, t1, a.id).await.unwrap());
    // 管理者刪除：回傳被刪 key 的擁有者與供應商（供通知與稽核，不含 key）
    let gone = store::delete_any(&pool, b.id).await.unwrap().unwrap();
    assert_eq!((gone.owner_id, gone.provider.as_str()), (t2, "openai"));
    assert!(store::delete_any(&pool, b.id).await.unwrap().is_none());
    // 刪除後同一把 key 可再被綁定
    store::add(&pool, &c, t2, "openai", KEY_A).await.unwrap();
}

#[sqlx::test]
async fn keys_for_returns_decrypted_keys_per_teacher_and_provider_in_creation_order(pool: PgPool) {
    let t1 = teacher(&pool, "t1@example.com").await;
    let t2 = teacher(&pool, "t2@example.com").await;
    let c = cipher();
    store::add(&pool, &c, t1, "openai", KEY_A).await.unwrap();
    store::add(
        &pool,
        &c,
        t1,
        "anthropic",
        "sk-synthetic-CCCC-1111-2222-3333-cccc",
    )
    .await
    .unwrap();
    store::add(&pool, &c, t1, "openai", KEY_B).await.unwrap();
    store::add(
        &pool,
        &c,
        t2,
        "openai",
        "sk-synthetic-DDDD-1111-2222-3333-dddd",
    )
    .await
    .unwrap();
    let keys = store::keys_for(&pool, &c, t1, "openai").await.unwrap();
    let secrets: Vec<&str> = keys.iter().map(|k| k.secret()).collect();
    assert_eq!(secrets, [KEY_A, KEY_B]);
    // Debug 不印出 key
    assert!(!format!("{keys:?}").contains("sk-synthetic"));
    assert!(
        store::keys_for(&pool, &c, t1, "gemini")
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn keys_encrypted_under_an_old_key_id_still_decrypt_after_rotation(pool: PgPool) {
    let t = teacher(&pool, "t1@example.com").await;
    store::add(&pool, &cipher(), t, "openai", KEY_A)
        .await
        .unwrap();
    let rotated = KeyCipher::parse(&format!("{ENC_2},{ENC_1}")).unwrap();
    let keys = store::keys_for(&pool, &rotated, t, "openai").await.unwrap();
    assert_eq!(keys[0].secret(), KEY_A);
    // 解密不了（金鑰設定錯）時跳過該筆並不洩漏內容，而不是讓整批失敗
    let wrong = KeyCipher::parse(ENC_2).unwrap();
    assert!(
        store::keys_for(&pool, &wrong, t, "openai")
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn status_records_reason_and_time_and_ok_clears_them(pool: PgPool) {
    let t = teacher(&pool, "t1@example.com").await;
    let k = store::add(&pool, &cipher(), t, "openai", KEY_A)
        .await
        .unwrap();
    assert!(k.status_at.is_none());
    for s in [
        KeyStatus::Invalid,
        KeyStatus::RateLimited,
        KeyStatus::QuotaExhausted,
    ] {
        assert!(store::set_status(&pool, k.id, s).await.unwrap());
        let got = &store::list_for_owner(&pool, t).await.unwrap()[0];
        assert_eq!(got.status, s);
        assert!(got.status_at.is_some());
    }
    store::set_status(&pool, k.id, KeyStatus::Ok).await.unwrap();
    let got = &store::list_for_owner(&pool, t).await.unwrap()[0];
    assert_eq!((got.status, got.status_at), (KeyStatus::Ok, None));
    assert!(
        !store::set_status(&pool, uuid::Uuid::new_v4(), KeyStatus::Ok)
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn database_guards_status_provider_and_fingerprint_shape(pool: PgPool) {
    let t = teacher(&pool, "t1@example.com").await;
    for (provider, status, fp_len) in [
        ("openai", "weird", 32),
        ("", "ok", 32),
        ("OpenAI", "ok", 32),
        ("openai", "ok", 31),
    ] {
        let r = sqlx::query("INSERT INTO teacher_api_keys(owner_id,provider,ciphertext,encryption_key_id,mask,fingerprint,status) VALUES($1,$2,'\\x00'::bytea,'k1','…',decode(repeat('ab',$3),'hex'),$4)")
            .bind(t).bind(provider).bind(fp_len).bind(status)
            .execute(&pool).await;
        assert!(r.is_err(), "{provider} {status} {fp_len}");
    }
}

#[sqlx::test]
async fn deleting_the_owner_removes_their_keys(pool: PgPool) {
    let t = teacher(&pool, "t1@example.com").await;
    store::add(&pool, &cipher(), t, "openai", KEY_A)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(t)
        .execute(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM teacher_api_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = false)]
async fn forward_migration_preserves_existing_users_and_is_repeatable(pool: PgPool) {
    let all = sqlx::migrate!();
    let legacy = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|m| m.version < 20261118000000)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();
    let t = teacher(&pool, "t1@example.com").await;
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id=$1")
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    store::add(&pool, &cipher(), t, "openai", KEY_A)
        .await
        .unwrap();
}
