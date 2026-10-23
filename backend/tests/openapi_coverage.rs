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

fn routes_in_source() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
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
