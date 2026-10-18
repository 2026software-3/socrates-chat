//! OpenAPI 文件（機器可讀的 API 契約），由各 handler 的 `#[utoipa::path]` 與型別的 `ToSchema` 產生。
//! 公開在 `GET /api/openapi.json`；新增或修改端點時，註解要跟著改（測試會檢查每條路由都有收錄）。

use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

use crate::{
    accounts, auth, catalog, chat, claims, dashboard,
    error::{CommonErrors, ErrorBody},
    reply, roster, summary, voice,
};

struct SessionCookie;

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
        auth::me, auth::reset_password,
        roster::list_teachers, roster::add_teacher, roster::remove_teacher, roster::list_roster,
        roster::import_roster, roster::remove_from_roster,
        accounts::list_students, accounts::disable_user, accounts::enable_user,
        accounts::delete_student,
        catalog::list_topics_for_teacher,
        catalog::create_topic_for_teacher,
        catalog::update_topic_for_teacher, catalog::list_activities, catalog::create_activity,
        catalog::get_activity, catalog::update_activity, catalog::publish_activity,
        catalog::close_activity, catalog::available,
        chat::create, chat::list, chat::detail, chat::remove, chat::send_message,
        reply::stream,
        summary::end, summary::get_summary, summary::retry, summary::teacher_list,
        dashboard::personal, dashboard::class, claims::regroup,
        voice::transcribe, voice::speech,
    ),
    components(schemas(ErrorBody)),
    modifiers(&SessionCookie),
    security(("session" = []))
)]
pub struct ApiDoc;

/// 目前所有 handler 註解組成的文件。
pub fn document() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[allow(unused)]
fn _common_errors_are_referenced(_: CommonErrors) {}
