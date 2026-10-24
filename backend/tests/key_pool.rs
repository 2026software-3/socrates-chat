//! KeyPool：輪詢分配、冷卻、停用，以及金鑰清單的解析。

use std::time::Duration;

use socrates_chat_backend::key_pool::{KeyPool, merge_keys, parse_key_lines};

fn pool(n: usize) -> KeyPool {
    KeyPool::new((1..=n).map(|i| format!("k{i}")).collect()).unwrap()
}

#[test]
fn empty_pool_is_rejected() {
    assert!(KeyPool::new(vec![]).is_none());
}

#[test]
fn acquire_rotates_round_robin() {
    let p = pool(3);
    let got: Vec<String> = (0..6).map(|_| p.acquire().unwrap().key).collect();
    assert_eq!(got, ["k1", "k2", "k3", "k1", "k2", "k3"]);
}

#[test]
fn rate_limited_key_is_skipped_until_cooldown_ends() {
    let p = pool(2);
    let first = p.acquire().unwrap();
    p.mark_rate_limited(first.index, Some(Duration::from_millis(80)));
    for _ in 0..4 {
        assert_eq!(p.acquire().unwrap().key, "k2");
    }
    std::thread::sleep(Duration::from_millis(120));
    let keys: Vec<String> = (0..2).map(|_| p.acquire().unwrap().key).collect();
    assert!(keys.contains(&"k1".to_string()), "{keys:?}");
}

#[test]
fn missing_retry_after_uses_default_cooldown() {
    let p = KeyPool::new(vec!["a".into()])
        .unwrap()
        .with_default_cooldown(Duration::from_millis(60));
    let l = p.acquire().unwrap();
    p.mark_rate_limited(l.index, None);
    assert!(p.acquire().is_none());
    std::thread::sleep(Duration::from_millis(100));
    assert!(p.acquire().is_some());
}

#[test]
fn default_cooldown_is_sixty_seconds() {
    let p = pool(1);
    let l = p.acquire().unwrap();
    p.mark_rate_limited(l.index, None);
    assert!(p.acquire().is_none());
    assert_eq!(p.default_cooldown(), Duration::from_secs(60));
}

#[test]
fn invalid_key_is_disabled_for_good() {
    let p = pool(2);
    let first = p.acquire().unwrap();
    p.mark_invalid(first.index);
    for _ in 0..5 {
        assert_eq!(p.acquire().unwrap().key, "k2");
    }
    assert_eq!(p.usable_count(), 1);
}

#[test]
fn acquire_returns_none_when_every_key_is_unavailable() {
    let p = pool(2);
    p.mark_invalid(0);
    p.mark_rate_limited(1, Some(Duration::from_secs(30)));
    assert!(p.acquire().is_none());
}

#[test]
fn rate_limit_does_not_resurrect_a_disabled_key() {
    let p = pool(1);
    p.mark_invalid(0);
    p.mark_rate_limited(0, Some(Duration::from_millis(1)));
    std::thread::sleep(Duration::from_millis(10));
    assert!(p.acquire().is_none());
}

#[test]
fn key_lines_ignore_blanks_comments_and_whitespace() {
    let raw = "# 主要\n  sk-a  \n\n\t\nsk-b\r\n#sk-c\nsk-d";
    assert_eq!(parse_key_lines(raw), ["sk-a", "sk-b", "sk-d"]);
}

#[test]
fn merge_keys_dedups_and_keeps_order() {
    let merged = merge_keys(vec![
        vec!["a".into(), "b".into()],
        vec!["b".into(), "c".into(), "a".into()],
    ]);
    assert_eq!(merged, ["a", "b", "c"]);
}

#[test]
fn debug_output_never_contains_keys() {
    let p = KeyPool::new(vec!["sk-secret-value".into()]).unwrap();
    assert!(!format!("{p:?}").contains("sk-secret"));
    let lease = p.acquire().unwrap();
    assert!(!format!("{lease:?}").contains("sk-secret"));
}
