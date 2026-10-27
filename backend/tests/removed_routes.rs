//! 已移除的路由：舊路徑仍須回 404（或 405），且 OpenAPI 文件不得再列出。
//! 新增「某路由已移除」的檢查時，加一列到 `REMOVED` 即可。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

/// 以哪個角色呼叫。
#[derive(Clone, Copy)]
enum As {
    Admin,
    Teacher,
    Student,
}

struct Removed {
    method: &'static str,
    /// `{topic}` 會換成實際存在的主題 id，確保 404 來自路由被移除而非 id 不存在。
    path: &'static str,
    as_: As,
    body: Option<&'static str>,
    /// 允許的狀態碼（路由不存在，或該路徑只剩其他方法時為 405）。
    statuses: &'static [StatusCode],
    /// OpenAPI 裡不得出現的 (path, method)。
    not_in_openapi: Option<(&'static str, &'static str)>,
}

const NOT_FOUND: &[StatusCode] = &[StatusCode::NOT_FOUND];

const REMOVED: &[Removed] = &[
    Removed {
        method: "GET",
        path: "/api/admin/topics",
        as_: As::Admin,
        body: Some(r#"{"title":"T"}"#),
        statuses: NOT_FOUND,
        not_in_openapi: Some(("/api/admin/topics", "get")),
    },
    Removed {
        method: "POST",
        path: "/api/admin/topics",
        as_: As::Admin,
        body: Some(r#"{"title":"T"}"#),
        statuses: NOT_FOUND,
        not_in_openapi: Some(("/api/admin/topics", "post")),
    },
    Removed {
        method: "PATCH",
        path: "/api/admin/topics/{topic}",
        as_: As::Admin,
        body: Some(r#"{"title":"T"}"#),
        statuses: NOT_FOUND,
        not_in_openapi: Some(("/api/admin/topics/{id}", "patch")),
    },
    Removed {
        method: "GET",
        path: "/api/available",
        as_: As::Student,
        body: None,
        statuses: NOT_FOUND,
        not_in_openapi: None,
    },
    Removed {
        method: "GET",
        path: "/api/teacher/summaries",
        as_: As::Teacher,
        body: None,
        statuses: NOT_FOUND,
        not_in_openapi: None,
    },
    Removed {
        method: "POST",
        path: "/api/students/student@example.com/delete",
        as_: As::Teacher,
        body: Some(r#"{"confirm_email":"student@example.com"}"#),
        statuses: &[StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED],
        not_in_openapi: None,
    },
];

#[sqlx::test]
async fn removed_routes_stay_removed(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/topics",
                Some(&a.admin),
                Some(json!({"title":"Before"})),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let openapi = json_body(send(&app, get("/api/openapi.json", None)).await).await;

    for r in REMOVED {
        let cookie = match r.as_ {
            As::Admin => &a.admin,
            As::Teacher => &a.teacher,
            As::Student => &a.student,
        };
        let path = r.path.replace("{topic}", &topic);
        let body = r.body.map(|b| serde_json::from_str(b).unwrap());
        let res = send(&app, json_req(r.method, &path, Some(cookie), body)).await;
        assert!(
            r.statuses.contains(&res.status()),
            "{} {} should be gone, got {}",
            r.method,
            r.path,
            res.status()
        );
        if let Some((doc_path, doc_method)) = r.not_in_openapi {
            assert!(
                openapi["paths"][doc_path][doc_method].is_null(),
                "OpenAPI still documents {doc_method} {doc_path}"
            );
        }
    }

    // 舊的刪除學生路由不得產生副作用：學生仍在、沒有任何稽核紀錄
    let students: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(students, 1);
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'student_deleted'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audits, 0);
}
