//! OpenAPI 文件（機器可讀的 API 契約），由各 handler 的 `#[utoipa::path]` 與型別的 `ToSchema` 產生。
//! 公開在 `GET /api/openapi.json`；新增或修改端點時，註解要跟著改（測試會檢查每條路由都有收錄）。

use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

use crate::{
    accounts, auth, catalog, chat, claims, classroom_store, classrooms, dashboard,
    error::ErrorBody, points, reply, request_logs, roster, roster_store, summary, voice,
};

struct SessionCookie;

/// 幾乎所有端點共有的錯誤回應，統一補上並以 `$ref` 指向 `ErrorBody`，避免每個端點重複展開：
/// 401／403 只加在需要登入或會改變狀態的端點；404 只加在有路徑參數的端點；500 一律加。
/// 端點自己宣告的同狀態碼（例如登入的 401 `invalid_credentials`）不會被覆蓋。
struct CommonErrorResponses;

impl Modify for CommonErrorResponses {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::{
            PathItem, Ref, RefOr, content::ContentBuilder, path::HttpMethod,
            response::ResponseBuilder,
        };
        let response = |description: &str| {
            RefOr::T(
                ResponseBuilder::new()
                    .description(description)
                    .content(
                        "application/json",
                        ContentBuilder::new()
                            .schema(Some(Ref::from_schema_name("ErrorBody")))
                            .build(),
                    )
                    .build(),
            )
        };
        for item in openapi.paths.paths.values_mut() {
            let has_path_params = |op: &utoipa::openapi::path::Operation| {
                op.parameters.iter().flatten().any(|p| {
                    matches!(
                        p,
                        RefOr::T(p) if matches!(p.parameter_in, utoipa::openapi::path::ParameterIn::Path)
                    )
                })
            };
            let PathItem {
                get,
                put,
                post,
                delete,
                patch,
                ..
            } = item;
            for (method, op) in [
                (HttpMethod::Get, get),
                (HttpMethod::Put, put),
                (HttpMethod::Post, post),
                (HttpMethod::Delete, delete),
                (HttpMethod::Patch, patch),
            ] {
                let Some(op) = op else { continue };
                // `security(())` 代表公開端點
                let public = op.security.as_ref().is_some_and(|s| {
                    s.iter()
                        .any(|r| r == &utoipa::openapi::security::SecurityRequirement::default())
                });
                let path_param = has_path_params(op);
                let mut add = |code: &str, description: &str| {
                    op.responses
                        .responses
                        .entry(code.to_string())
                        .or_insert_with(|| response(description));
                };
                if !public {
                    add("401", "未登入（`unauthorized`）");
                }
                if !public || method != HttpMethod::Get {
                    add(
                        "403",
                        "角色不符、名單外、帳號停用、須先改密碼或 CSRF（`forbidden`／`not_enrolled`／`account_disabled`／`password_change_required`／`csrf`）",
                    );
                }
                if path_param {
                    add("404", "資源不存在或不屬於你（`not_found`）");
                }
                add("500", "系統錯誤（`internal`）");
            }
        }
    }
}

impl Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("sid"))),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Socrates Chat API",
        description = "蘇格拉底式對話學習系統的後端 API。所有路徑在同一網域的 /api 之下；認證靠 sid HttpOnly cookie；\
                       會改變狀態的請求必須帶等於 APP_BASE_URL 的 Origin 標頭，否則 403 csrf。\
                       錯誤格式為 { error: { code, request_id } }。"
    ),
    paths(
        auth::login, auth::callback, auth::password_login, auth::change_password, auth::logout,
        auth::me, auth::update_me, auth::reset_password,
        classrooms::list_classrooms, classrooms::create_classroom,
        classrooms::get_classroom, classrooms::update_classroom, classrooms::delete_classroom,
        roster::list_teachers, roster::add_teacher, roster::remove_teacher, roster::list_roster,
        roster::import_roster, roster::remove_from_roster,
        accounts::list_students, accounts::set_status,
        accounts::delete_student,
        catalog::list_topics_for_teacher,
        catalog::create_topic_for_teacher,
        catalog::update_topic_for_teacher, catalog::list_activities, catalog::create_activity,
        catalog::get_activity, catalog::update_activity, catalog::publish_activity,
        catalog::close_activity, catalog::archive_activity, catalog::unarchive_activity,
        catalog::delete_activity, catalog::available,
        chat::create, chat::list, chat::detail, chat::remove, chat::archive, chat::unarchive, chat::send_message,
        reply::stream,
        request_logs::list,
        summary::end, summary::get_summary, summary::retry, summary::list,
        dashboard::personal, dashboard::class, claims::regroup,
        voice::transcribe, voice::speech,
        points::mine,
    ),
    components(schemas(ErrorBody, classroom_store::Classroom, roster_store::StudentRow)),
    modifiers(&SessionCookie, &CommonErrorResponses),
    security(("session" = []))
)]
pub struct ApiDoc;

/// 目前所有 handler 註解組成的文件。
pub fn document() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}
