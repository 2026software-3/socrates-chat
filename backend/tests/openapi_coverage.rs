//! 每條路由都要有 OpenAPI 註解（反過來也一樣）。
//!
//! `openapi.rs` 的測試只能從文件出發檢查路由存在；這裡從原始碼的 `.route(` 出發，
//! 找出「新增了路由卻忘了加 `#[utoipa::path]`（或忘了列進 `ApiDoc`）」的情況。
//! 不需要資料庫。

use std::{collections::BTreeSet, fs, path::Path};

use socrates_chat_backend::openapi;

/// 文件之外的路由：健康檢查與文件本身（含 API reference 頁面用的 Redoc 腳本）。
const UNDOCUMENTED: [&str; 3] = [
    "/health",
    "/api/openapi.json",
    "/api/docs/redoc.standalone.js",
];
const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// 取出 `open` 位置（`(`）對應的右括號之前的內容。
fn balanced(src: &str, open: usize) -> &str {
    let mut depth = 0;
    for (i, c) in src[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced parentheses after `.route(`");
}

/// 方法呼叫 `get(`／`post(`…（單字邊界，所以 `get_summary(` 不算）。
fn methods_in(chain: &str) -> Vec<&'static str> {
    let bytes = chain.as_bytes();
    METHODS
        .into_iter()
        .filter(|m| {
            chain.match_indices(&format!("{m}(")).any(|(i, _)| {
                i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
            })
        })
        .collect()
}

/// 去掉 `#[cfg(test)]` 模組：測試裡用的 `.route(` 不是正式路由，不該被當成需要文件的路由。
/// 模組不一定在檔案最後，所以以大括號配對找出結尾，只移除模組本身。
fn without_test_modules(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let block = &rest[at..];
        rest = &block[matching_brace_end(block)..];
    }
    out.push_str(rest);
    out
}

/// `block` 中第一個 `{` 所對應的 `}` 之後的位置；找不到時為結尾。
/// 字串、raw string 與字元常值裡的大括號不算（生命週期的 `'a` 不是字元常值）。
fn matching_brace_end(block: &str) -> usize {
    let b = block.as_bytes();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = block.find('{').unwrap_or(b.len());
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'r' if i > 0 && !is_ident(b[i - 1]) => {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let body = i + 2 + hashes;
                    i = block[body..]
                        .find(&close)
                        .map_or(b.len(), |n| body + n + close.len() - 1);
                }
            }
            b'\'' => {
                if b.get(i + 1) == Some(&b'\\') {
                    // 跳脫的字元常值：`'\''`、`'\n'`、`'\u{7b}'`
                    i += 3;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                } else if let Some(c) = block[i + 1..].chars().next()
                    && b.get(i + 1 + c.len_utf8()) == Some(&b'\'')
                {
                    i += c.len_utf8() + 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

fn routes_in_source() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let src = without_test_modules(&fs::read_to_string(&path).unwrap());
        for (i, _) in src.match_indices(".route(") {
            let args = balanced(&src, i + ".route".len());
            let start = args.find('"').expect("route path literal") + 1;
            let end = start + args[start..].find('"').unwrap();
            let route_path = &args[start..end];
            if UNDOCUMENTED.contains(&route_path) {
                continue;
            }
            let methods = methods_in(&args[end..]);
            assert!(
                !methods.is_empty(),
                "no method found for route {route_path}"
            );
            for m in methods {
                out.insert(format!("{} {route_path}", m.to_uppercase()));
            }
        }
    }
    out
}

fn operations_in_openapi() -> BTreeSet<String> {
    let doc = serde_json::to_value(openapi::document()).unwrap();
    let mut out = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            out.insert(format!("{} {path}", method.to_uppercase()));
        }
    }
    out
}

#[test]
fn every_route_is_documented_in_openapi_and_vice_versa() {
    let routes = routes_in_source();
    let documented = operations_in_openapi();
    assert!(routes.len() >= 40, "route scan found only {}", routes.len());

    let missing: Vec<_> = routes.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&routes).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "OpenAPI is out of sync with the routes.\n\
         routed but not documented (add #[utoipa::path] and list it in openapi.rs `paths(...)`): {missing:#?}\n\
         documented but not routed (remove the annotation or fix its method/path): {stale:#?}"
    );
}

#[test]
fn test_module_stripping_ignores_braces_inside_literals() {
    let src = concat!(
        "fn a() {}\n#[cfg(test)]\nmod t {\n",
        "    const A: &str = \"{\";\n    const B: char = '}';\n    const C: char = '\\'';\n",
        "    fn x<'a>(s: &'a str) { let _ = r#\"{{\"#; }\n}\n",
        "fn real() { Router::new().route(\"/x\", get(h)) }\n"
    );
    let out = without_test_modules(src);
    assert!(out.contains(".route(\"/x\""), "{out}");
    assert!(!out.contains("const A"), "{out}");
}
