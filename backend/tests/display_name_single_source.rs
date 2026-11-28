//! #548：顯示名稱只能經 `display_name_sql!` 解析；原始碼不得再以 `<別名>.display_name` 直接讀取該欄位。

use std::{fs, path::Path};

mod common;

/// SQL 字串內的 `<別名>.display_name`（Rust 結構欄位存取不在引號內，不受影響）。
fn raw_reads(line: &str) -> bool {
    line.split('"').skip(1).step_by(2).any(|literal| {
        literal.match_indices(".display_name").any(|(i, _)| {
            let before = literal[..i].chars().next_back();
            let after = literal[i + ".display_name".len()..].chars().next();
            before.is_some_and(|c| c.is_ascii_alphabetic())
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}

#[test]
fn no_query_reads_the_raw_display_name_column() {
    let files = common::rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    let mut offenders = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if raw_reads(line) {
                offenders.push(format!("{}:{}", file.display(), n + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "raw display_name reads: {offenders:?}"
    );
}

#[test]
fn detector_flags_a_raw_read() {
    assert!(raw_reads(r#"    "SELECT u.display_name FROM users u","#));
    assert!(!raw_reads("    let name = p.display_name;"));
}
