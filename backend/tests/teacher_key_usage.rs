//! F-40.8-DB (#602): 教師 key 的 AI 用量累計與彙總；資料皆為合成。
mod common;

use common::*;
use socrates_chat_backend::teacher_key_crypto::KeyCipher;
use socrates_chat_backend::teacher_key_store as keys;
use socrates_chat_backend::teacher_key_usage::{self as usage, Range, Totals};
use sqlx::PgPool;
use uuid::Uuid;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

struct World {
    teacher: Uuid,
    key: Uuid,
    classroom: Uuid,
}

async fn classroom(pool: &PgPool, owner: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO classrooms (name, created_by) VALUES ($1, $2) RETURNING id")
        .bind(name)
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn add_key(pool: &PgPool, owner: Uuid, secret: &str) -> Uuid {
    keys::add(
        pool,
        &KeyCipher::parse(ENC).unwrap(),
        owner,
        "openai",
        secret,
    )
    .await
    .unwrap()
    .id
}

async fn world(pool: &PgPool) -> World {
    let teacher = seed_user(pool, "t@example.com").await;
    let key = add_key(pool, teacher, "sk-synthetic-AAAA-1111-2222-3333-aaaa").await;
    let classroom = classroom(pool, teacher, "c1").await;
    World {
        teacher,
        key,
        classroom,
    }
}

/// 直接寫入指定「日」的列，用來測時間範圍；`days_ago` 相對於 Asia/Taipei 的今天。
async fn row(pool: &PgPool, w: &World, days_ago: i32, requests: i64, input: i64, output: i64) {
    sqlx::query(
        "INSERT INTO teacher_key_usage (key_id, classroom_id, provider, model, day, requests, input_tokens, output_tokens)
         VALUES ($1, $2, 'openai', 'm', (now() AT TIME ZONE 'Asia/Taipei')::date - $3, $4, $5, $6)",
    )
    .bind(w.key).bind(w.classroom).bind(days_ago).bind(requests).bind(input).bind(output)
    .execute(pool).await.unwrap();
}

fn t(requests: i64, input: i64, output: i64, unreported: i64) -> Totals {
    Totals {
        requests,
        input_tokens: input,
        output_tokens: output,
        unreported_requests: unreported,
    }
}

#[sqlx::test]
async fn record_accumulates_per_key_classroom_model_and_day(pool: PgPool) {
    let w = world(&pool).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((3, 2)))
        .await
        .unwrap();
    usage::record(&pool, w.key, w.classroom, "openai", "other", Some((1, 1)))
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM teacher_key_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
    let got = usage::totals_for_key(&pool, w.key, Range::Today)
        .await
        .unwrap();
    assert_eq!(got, t(3, 14, 8, 0));
}

#[sqlx::test]
async fn missing_token_report_counts_a_request_without_estimating_tokens(pool: PgPool) {
    let w = world(&pool).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    usage::record(&pool, w.key, w.classroom, "openai", "m", None)
        .await
        .unwrap();
    assert_eq!(
        usage::totals_for_key(&pool, w.key, Range::All)
            .await
            .unwrap(),
        t(2, 10, 5, 1)
    );
}

#[sqlx::test]
async fn negative_or_oversized_token_counts_are_rejected_not_stored(pool: PgPool) {
    let w = world(&pool).await;
    assert!(
        usage::record(&pool, w.key, w.classroom, "openai", "m", Some((-1, 0)))
            .await
            .is_err()
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM teacher_key_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn ranges_cover_today_7_days_30_days_and_all_with_exact_boundaries(pool: PgPool) {
    let w = world(&pool).await;
    for (ago, r) in [
        (0, 1),
        (1, 10),
        (6, 100),
        (7, 1000),
        (29, 10_000),
        (30, 100_000),
        (400, 1_000_000),
    ] {
        row(&pool, &w, ago, r, r, r).await;
    }
    let req = |range| {
        let pool = pool.clone();
        async move {
            usage::totals_for_key(&pool, w.key, range)
                .await
                .unwrap()
                .requests
        }
    };
    assert_eq!(req(Range::Today).await, 1);
    // 最近 7 天含今天，共 7 個日曆日（今天與前 6 天）
    assert_eq!(req(Range::Last7Days).await, 111);
    assert_eq!(req(Range::Last30Days).await, 11_111);
    assert_eq!(req(Range::All).await, 1_111_111);
}

#[sqlx::test]
async fn classroom_totals_sum_all_keys_and_other_classrooms_are_excluded(pool: PgPool) {
    let w = world(&pool).await;
    let k2 = add_key(&pool, w.teacher, "sk-synthetic-BBBB-1111-2222-3333-bbbb").await;
    let c2 = classroom(&pool, w.teacher, "c2").await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    usage::record(&pool, k2, w.classroom, "openai", "m", Some((1, 1)))
        .await
        .unwrap();
    usage::record(&pool, k2, c2, "openai", "m", Some((100, 100)))
        .await
        .unwrap();
    assert_eq!(
        usage::totals_for_classroom(&pool, w.classroom, Range::All)
            .await
            .unwrap(),
        t(2, 11, 6, 0)
    );
    assert_eq!(
        usage::totals_for_classroom(&pool, c2, Range::All)
            .await
            .unwrap(),
        t(1, 100, 100, 0)
    );
    assert_eq!(
        usage::totals_for_key(&pool, k2, Range::All).await.unwrap(),
        t(2, 101, 101, 0)
    );
}

#[sqlx::test]
async fn empty_usage_is_zero_not_an_error(pool: PgPool) {
    let w = world(&pool).await;
    assert_eq!(
        usage::totals_for_key(&pool, w.key, Range::All)
            .await
            .unwrap(),
        t(0, 0, 0, 0)
    );
    assert_eq!(
        usage::totals_for_classroom(&pool, w.classroom, Range::All)
            .await
            .unwrap(),
        t(0, 0, 0, 0)
    );
}

#[sqlx::test]
async fn deleting_a_key_removes_its_usage_and_the_classroom_total_drops(pool: PgPool) {
    let w = world(&pool).await;
    let k2 = add_key(&pool, w.teacher, "sk-synthetic-BBBB-1111-2222-3333-bbbb").await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    usage::record(&pool, k2, w.classroom, "openai", "m", Some((1, 1)))
        .await
        .unwrap();
    assert!(keys::delete(&pool, w.teacher, w.key).await.unwrap());
    assert_eq!(
        usage::totals_for_key(&pool, w.key, Range::All)
            .await
            .unwrap(),
        t(0, 0, 0, 0)
    );
    assert_eq!(
        usage::totals_for_classroom(&pool, w.classroom, Range::All)
            .await
            .unwrap(),
        t(1, 1, 1, 0)
    );
}

#[sqlx::test]
async fn replacing_the_key_secret_keeps_its_history(pool: PgPool) {
    let w = world(&pool).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    let c = KeyCipher::parse(ENC).unwrap();
    keys::replace(
        &pool,
        &c,
        w.teacher,
        w.key,
        "sk-synthetic-CCCC-1111-2222-3333-cccc",
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        usage::totals_for_key(&pool, w.key, Range::All)
            .await
            .unwrap(),
        t(1, 10, 5, 0)
    );
    assert_eq!(
        usage::totals_for_classroom(&pool, w.classroom, Range::All)
            .await
            .unwrap(),
        t(1, 10, 5, 0)
    );
}

#[sqlx::test]
async fn deleting_a_classroom_removes_its_usage_but_not_the_key(pool: PgPool) {
    let w = world(&pool).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    sqlx::query("DELETE FROM classrooms WHERE id=$1")
        .bind(w.classroom)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        usage::totals_for_key(&pool, w.key, Range::All)
            .await
            .unwrap(),
        t(0, 0, 0, 0)
    );
    assert_eq!(
        keys::list_for_owner(&pool, w.teacher).await.unwrap().len(),
        1
    );
}

#[sqlx::test]
async fn schema_has_no_money_columns(pool: PgPool) {
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns WHERE table_name='teacher_key_usage' ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(!cols.is_empty());
    for c in &cols {
        assert!(
            !["cost", "price", "amount", "usd", "twd", "money"]
                .iter()
                .any(|m| c.contains(m)),
            "{c}"
        );
    }
}
