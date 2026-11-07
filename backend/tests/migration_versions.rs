//! migration 版本號必須唯一。
//!
//! sqlx 以檔名開頭的數字當版本號，並把它當 `_sqlx_migrations` 的主鍵。兩個檔案版本號相同時，
//! 後套用的那個會因 duplicate key 而失敗，後端無法啟動；#467 與 #469 曾各自在自己的分支上通過 CI，
//! 合併後才一起壞掉。這個測試不連資料庫，在合併前就擋下來。

use std::{collections::BTreeMap, fs, path::Path};

/// 回傳 `版本號 → 檔名`，只列出出現不只一次的版本號。
/// 版本號比照 sqlx，取第一個 `_` 之前的數字（`01` 與 `1` 視為同一個版本）。
fn duplicate_versions(dir: &Path) -> BTreeMap<i64, Vec<String>> {
    let mut by_version: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if !name.ends_with(".sql") {
            continue;
        }
        let version = name
            .split('_')
            .next()
            .and_then(|prefix| prefix.parse::<i64>().ok())
            .unwrap_or_else(|| panic!("migration 檔名要以數字版本號開頭：{name}"));
        by_version.entry(version).or_default().push(name);
    }
    by_version.retain(|_, files| files.len() > 1);
    for files in by_version.values_mut() {
        files.sort();
    }
    by_version
}

#[test]
fn migration_versions_are_unique() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let duplicates = duplicate_versions(&dir);
    assert!(
        duplicates.is_empty(),
        "migration 版本號重複（sqlx 以版本號為主鍵，後套用的會失敗）；請把較晚合併的改成更大的版本號：{duplicates:#?}"
    );
}

#[test]
fn duplicate_versions_are_detected() {
    let dir = std::env::temp_dir().join(format!(
        "migration-versions-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    for name in [
        "20261001000000_create_a.sql",
        "20261107000000_create_b.sql",
        "20261107000000_create_c.sql",
        "0000000000001_with_leading_zeros.sql",
        "1_same_version_as_leading_zeros.sql",
        "notes.md",
    ] {
        fs::write(dir.join(name), "").unwrap();
    }

    let duplicates = duplicate_versions(&dir);
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(
        duplicates,
        BTreeMap::from([
            (
                1,
                vec![
                    "0000000000001_with_leading_zeros.sql".to_owned(),
                    "1_same_version_as_leading_zeros.sql".to_owned(),
                ]
            ),
            (
                20261107000000,
                vec![
                    "20261107000000_create_b.sql".to_owned(),
                    "20261107000000_create_c.sql".to_owned(),
                ]
            ),
        ])
    );
}
