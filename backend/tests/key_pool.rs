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
fn rejected_key_cools_down_then_recovers() {
    let p = pool(2).with_auth_cooldown(Duration::from_millis(80));
    let first = p.acquire().unwrap();
    p.mark_rejected(first.index);
    for _ in 0..4 {
        assert_eq!(p.acquire().unwrap().key, "k2");
    }
    assert_eq!(p.usable_count(), 1);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(p.usable_count(), 2);
    let keys: Vec<String> = (0..2).map(|_| p.acquire().unwrap().key).collect();
    assert!(keys.contains(&"k1".to_string()), "{keys:?}");
}

#[test]
fn default_auth_cooldown_is_ten_minutes() {
    assert_eq!(pool(1).auth_cooldown(), Duration::from_secs(600));
}

#[test]
fn single_rejected_key_recovers_instead_of_staying_dead() {
    let p = KeyPool::new(vec!["only".into()])
        .unwrap()
        .with_auth_cooldown(Duration::from_millis(60));
    p.mark_rejected(0);
    assert!(p.acquire().is_none());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(p.acquire().unwrap().key, "only");
}

#[test]
fn acquire_returns_none_when_every_key_is_unavailable() {
    let p = pool(2);
    p.mark_rejected(0);
    p.mark_rate_limited(1, Some(Duration::from_secs(30)));
    assert!(p.acquire().is_none());
}

#[test]
fn rate_limit_never_shortens_a_longer_cooldown() {
    let p = pool(1);
    p.mark_rejected(0);
    p.mark_rate_limited(0, Some(Duration::from_millis(1)));
    std::thread::sleep(Duration::from_millis(10));
    assert!(p.acquire().is_none());
}

#[test]
fn zero_retry_after_still_cools_the_key_for_a_moment() {
    let p = pool(1);
    p.mark_rate_limited(0, Some(Duration::ZERO));
    assert!(p.acquire().is_none());
    let wait = p.next_available_in().unwrap();
    assert!(
        wait > Duration::from_millis(500) && wait <= Duration::from_secs(1),
        "{wait:?}"
    );
}

#[test]
fn next_available_in_reports_the_soonest_recovery() {
    let p = pool(3);
    assert_eq!(p.next_available_in(), Some(Duration::ZERO));
    p.mark_rate_limited(0, Some(Duration::from_secs(30)));
    p.mark_rate_limited(1, Some(Duration::from_secs(5)));
    assert_eq!(p.next_available_in(), Some(Duration::ZERO), "k3 is ready");
    p.mark_rate_limited(2, Some(Duration::from_secs(20)));
    let wait = p.next_available_in().unwrap();
    assert!(
        wait > Duration::from_secs(4) && wait <= Duration::from_secs(5),
        "{wait:?}"
    );
}

#[test]
fn concurrent_acquires_are_spread_evenly() {
    let p = std::sync::Arc::new(pool(3));
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let p = p.clone();
            std::thread::spawn(move || {
                (0..50)
                    .map(|_| p.acquire().unwrap().index)
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut counts = [0usize; 3];
    for h in handles {
        for i in h.join().unwrap() {
            counts[i] += 1;
        }
    }
    assert_eq!(counts, [100, 100, 100]);
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

#[test]
fn fork_has_the_same_keys_and_settings_but_independent_cooldowns() {
    let original = pool(2)
        .with_default_cooldown(Duration::from_secs(7))
        .with_auth_cooldown(Duration::from_secs(70));
    let forked = original.fork();
    assert_eq!(forked.len(), 2);
    assert_eq!(forked.default_cooldown(), Duration::from_secs(7));
    assert_eq!(forked.auth_cooldown(), Duration::from_secs(70));
    // 在原本的池子冷卻一把，不影響分出去的池子
    let lease = original.acquire().unwrap();
    original.mark_rate_limited(lease.index, None);
    assert_eq!(original.usable_count(), 1);
    assert_eq!(forked.usable_count(), 2);
    // 反過來也一樣，而且分出去時原本池子的冷卻狀態不會被帶過去
    let lease = forked.acquire().unwrap();
    forked.mark_rejected(lease.index);
    assert_eq!(original.usable_count(), 1);
    assert_eq!(forked.usable_count(), 1);
}

#[test]
fn only_rate_limited_cooldowns_count_as_rate_limited() {
    let pool = KeyPool::new(vec!["a".into(), "b".into()]).unwrap();
    assert!(!pool.has_rate_limited_key());
    pool.mark_rejected(0);
    assert!(!pool.has_rate_limited_key());
    pool.mark_rate_limited(1, None);
    assert!(pool.has_rate_limited_key());
}
