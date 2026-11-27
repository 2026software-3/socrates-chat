//! 語音節點管理 API（S-19 7.1，僅系統管理者）。環境節點只在列表出現，不能改也不能刪。
//!
//! 節點密鑰是 32 個隨機位元組的十六進位字串，只在建立與輪換的回應出現一次；資料庫只存 SHA-256。
//! 稽核只記操作者、動作、節點 `id` 與名稱，不記網址與密鑰。

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use rand::Rng;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    AppState, accounts,
    auth::RequireAdmin,
    error::{ApiError, ErrorBody},
    voice_nodes::{ENV_NODE, NodeId, NodeStatus},
};

const MAX_NAME_CHARS: usize = 100;
const MAX_SESSIONS_LIMIT: i64 = 10_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/voice-nodes", get(list).post(create))
        .route(
            "/api/admin/voice-nodes/{id}",
            axum::routing::patch(update).delete(remove),
        )
        .route(
            "/api/admin/voice-nodes/{id}/rotate-token",
            post(rotate_token),
        )
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct VoiceNodeView {
    /// 環境節點固定是 `env`；其他是節點的 UUID
    id: String,
    name: String,
    realtime_url: String,
    /// `active`／`draining`／`disabled`
    status: String,
    /// 最近的探測結果
    healthy: bool,
    /// 目前進行中的場次數（這個程序的記憶體，重啟歸零）
    sessions: usize,
    /// 環境節點未設上限時是 `null`
    max_sessions: Option<i64>,
    /// 環境節點由環境變數決定，不能改也不能刪
    read_only: bool,
}

/// 建立或輪換密鑰的回應：`token` 只出現這一次。
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct VoiceNodeWithToken {
    node: VoiceNodeView,
    /// 節點密鑰明文（32 個隨機位元組的十六進位）；之後任何回應都不含
    token: String,
}

type Row = (Uuid, String, String, i32, String);

fn view(state: &AppState, row: Row) -> VoiceNodeView {
    let (id, name, realtime_url, max_sessions, status) = row;
    let node = NodeId::Db(id);
    VoiceNodeView {
        id: id.to_string(),
        name,
        realtime_url,
        status,
        healthy: state.voice_nodes.is_healthy(node),
        sessions: state
            .voice_sessions
            .counts()
            .get(&node)
            .copied()
            .unwrap_or(0),
        max_sessions: Some(i64::from(max_sessions)),
        read_only: false,
    }
}

/// SQL 要是字面字串，欄位清單用巨集展開。
macro_rules! columns {
    () => {
        "id, name, realtime_url, max_sessions, status"
    };
}

#[utoipa::path(
    get,
    path = "/api/admin/voice-nodes",
    tag = "admin",
    summary = "列出語音節點（含環境節點）",
    responses(
        (status = 200, body = [VoiceNodeView]),
        (status = 401, description = "未登入", body = ErrorBody),
        (status = 403, description = "不是管理者", body = ErrorBody)
    )
)]
pub(crate) async fn list(
    RequireAdmin(_): RequireAdmin,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let rows: Vec<Row> = sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM voice_nodes ORDER BY name"
    ))
    .fetch_all(&state.pool)
    .await?;
    let mut items = Vec::new();
    if let Some(env) = crate::voice_nodes::Node::env(&state.config) {
        items.push(VoiceNodeView {
            id: ENV_NODE.into(),
            name: env.name,
            realtime_url: env.realtime_url,
            status: NodeStatus::Active.as_str().into(),
            healthy: state.voice_nodes.is_healthy(NodeId::Env),
            sessions: state
                .voice_sessions
                .counts()
                .get(&NodeId::Env)
                .copied()
                .unwrap_or(0),
            max_sessions: env.max_sessions.map(|n| n as i64),
            read_only: true,
        });
    }
    items.extend(rows.into_iter().map(|row| view(&state, row)));
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(items)))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NewNode {
    name: String,
    /// `ws://` 或 `wss://` 的 `/v1/realtime`
    realtime_url: String,
    /// 必填，≥ 1（建議起始值 1，暫定）
    max_sessions: i64,
}

fn invalid_body(_: JsonRejection) -> ApiError {
    ApiError::bad_request("invalid_request", "請求內容不正確")
}

fn clean_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
        || name == ENV_NODE
    {
        return Err(ApiError::bad_request("invalid_name", "節點名稱不符"));
    }
    Ok(name.to_string())
}

/// 內部主機名稱的後綴：容器／區網常用的名稱，加上 tailnet 的 MagicDNS（`.ts.net`，解析結果是 tailnet 位址）。
const INTERNAL_SUFFIXES: [&str; 5] = [".internal", ".local", ".lan", ".localdomain", ".ts.net"];

/// 只判斷寫法，不做 DNS 解析：IP 必須是私有、迴路、link-local、CGNAT（tailnet）；
/// 主機名稱必須沒有點（例如 compose 服務名 `cascade`）或以內部後綴結尾。
fn is_internal_host(host: &str) -> bool {
    use std::net::IpAddr;
    // `Url` 已把 134744072、0x7f.1 這類寫法正規化成點分十進位；IPv6 帶方括號
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (a == 100 && (64..=127).contains(&b))
        }
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
        Err(_) => {
            let name = host.trim_end_matches('.').to_ascii_lowercase();
            !name.is_empty()
                && (!name.contains('.')
                    || INTERNAL_SUFFIXES
                        .iter()
                        .any(|suffix| name.ends_with(suffix)))
        }
    }
}

fn clean_url(raw: &str) -> Result<String, ApiError> {
    let url = raw.trim();
    let parsed = Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "ws" | "wss") && u.has_host())
        .filter(|_| !url.chars().any(char::is_control))
        .ok_or_else(|| {
            ApiError::bad_request("invalid_realtime_url", "網址必須是 ws:// 或 wss://")
        })?;
    if !parsed.host_str().is_some_and(is_internal_host) {
        return Err(ApiError::bad_request(
            "realtime_url_not_internal",
            "節點網址必須是內網位址或內部主機名稱",
        ));
    }
    Ok(url.to_string())
}

fn clean_max(raw: i64) -> Result<i32, ApiError> {
    if (1..=MAX_SESSIONS_LIMIT).contains(&raw) {
        Ok(raw as i32)
    } else {
        Err(ApiError::bad_request(
            "invalid_max_sessions",
            "場次上限必須是 1 以上的整數",
        ))
    }
}

fn name_taken(e: sqlx::Error) -> ApiError {
    if e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
    {
        ApiError::conflict("voice_node_name_taken", "節點名稱已被使用")
    } else {
        e.into()
    }
}

/// 32 個隨機位元組的十六進位，與它的 SHA-256。
fn new_token() -> (String, [u8; 32]) {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let hash = Sha256::digest(token.as_bytes()).into();
    (token, hash)
}

/// 稽核的對象：節點 `id` 與名稱（不含網址與密鑰）。
async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    id: Uuid,
    name: &str,
) -> Result<(), sqlx::Error> {
    accounts::audit(conn, actor, action, &format!("{id} {name}"), None).await
}

#[utoipa::path(
    post,
    path = "/api/admin/voice-nodes",
    tag = "admin",
    summary = "新增語音節點；回應僅此一次含節點密鑰明文",
    request_body = NewNode,
    responses(
        (status = 201, body = VoiceNodeWithToken),
        (status = 400, description = "invalid_name／invalid_realtime_url／invalid_max_sessions／invalid_request", body = ErrorBody),
        (status = 401, description = "未登入", body = ErrorBody),
        (status = 403, description = "不是管理者", body = ErrorBody),
        (status = 409, description = "voice_node_name_taken", body = ErrorBody)
    )
)]
pub(crate) async fn create(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    body: Result<Json<NewNode>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(body) = body.map_err(invalid_body)?;
    let name = clean_name(&body.name)?;
    let url = clean_url(&body.realtime_url)?;
    let max = clean_max(body.max_sessions)?;
    let (token, hash) = new_token();
    let mut tx = state.pool.begin().await?;
    let row: Row = sqlx::query_as(concat!(
        "INSERT INTO voice_nodes (name, realtime_url, token_hash, max_sessions)
         VALUES ($1, $2, $3, $4) RETURNING ",
        columns!()
    ))
    .bind(&name)
    .bind(&url)
    .bind(hash.as_slice())
    .bind(max)
    .fetch_one(&mut *tx)
    .await
    .map_err(name_taken)?;
    audit(&mut tx, cu.user.id, "voice_node_create", row.0, &name).await?;
    tx.commit().await?;
    state.voice_nodes.invalidate();
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(VoiceNodeWithToken {
            node: view(&state, row),
            token,
        }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NodePatch {
    name: Option<String>,
    realtime_url: Option<String>,
    max_sessions: Option<i64>,
    /// `active`／`draining`／`disabled`
    status: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/admin/voice-nodes/{id}",
    tag = "admin",
    summary = "修改語音節點的名稱、網址、場次上限或狀態",
    params(("id" = Uuid, Path)),
    request_body = NodePatch,
    responses(
        (status = 200, body = VoiceNodeView),
        (status = 400, description = "invalid_name／invalid_realtime_url／invalid_max_sessions／invalid_status／invalid_request", body = ErrorBody),
        (status = 401, description = "未登入", body = ErrorBody),
        (status = 403, description = "不是管理者", body = ErrorBody),
        (status = 404, description = "節點不存在", body = ErrorBody),
        (status = 409, description = "voice_node_name_taken", body = ErrorBody)
    )
)]
pub(crate) async fn update(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<NodePatch>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(body) = body.map_err(invalid_body)?;
    let name = body.name.as_deref().map(clean_name).transpose()?;
    let url = body.realtime_url.as_deref().map(clean_url).transpose()?;
    let max = body.max_sessions.map(clean_max).transpose()?;
    let status = body
        .status
        .as_deref()
        .map(|s| {
            NodeStatus::parse(s)
                .ok_or(ApiError::bad_request("invalid_status", "狀態不符"))
                .map(NodeStatus::as_str)
        })
        .transpose()?;
    let mut tx = state.pool.begin().await?;
    let row: Option<Row> = sqlx::query_as(concat!(
        "UPDATE voice_nodes SET
             name = COALESCE($2, name),
             realtime_url = COALESCE($3, realtime_url),
             max_sessions = COALESCE($4, max_sessions),
             status = COALESCE($5, status),
             updated_at = now()
         WHERE id = $1 RETURNING ",
        columns!()
    ))
    .bind(id)
    .bind(name)
    .bind(url.as_deref())
    .bind(max)
    .bind(status)
    .fetch_optional(&mut *tx)
    .await
    .map_err(name_taken)?;
    let row = row.ok_or_else(ApiError::not_found)?;
    audit(&mut tx, cu.user.id, "voice_node_update", id, &row.1).await?;
    tx.commit().await?;
    if url.is_some() {
        state.voice_nodes.forget(NodeId::Db(id));
    }
    state.voice_nodes.invalidate();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(view(&state, row)),
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/voice-nodes/{id}/rotate-token",
    tag = "admin",
    summary = "換新節點密鑰，舊的立即失效；回應僅此一次含明文",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = VoiceNodeWithToken),
        (status = 401, description = "未登入", body = ErrorBody),
        (status = 403, description = "不是管理者", body = ErrorBody),
        (status = 404, description = "節點不存在", body = ErrorBody)
    )
)]
pub(crate) async fn rotate_token(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let (token, hash) = new_token();
    let mut tx = state.pool.begin().await?;
    let row: Option<Row> = sqlx::query_as(concat!(
        "UPDATE voice_nodes SET token_hash = $2, updated_at = now() WHERE id = $1 RETURNING ",
        columns!()
    ))
    .bind(id)
    .bind(hash.as_slice())
    .fetch_optional(&mut *tx)
    .await?;
    let row = row.ok_or_else(ApiError::not_found)?;
    audit(&mut tx, cu.user.id, "voice_node_rotate_token", id, &row.1).await?;
    tx.commit().await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(VoiceNodeWithToken {
            node: view(&state, row),
            token,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/admin/voice-nodes/{id}",
    tag = "admin",
    summary = "移除語音節點；密鑰立即失效，進行中的場次在一個快取週期內結束",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "已移除"),
        (status = 401, description = "未登入", body = ErrorBody),
        (status = 403, description = "不是管理者", body = ErrorBody),
        (status = 404, description = "節點不存在", body = ErrorBody)
    )
)]
pub(crate) async fn remove(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    let name: Option<String> =
        sqlx::query_scalar("DELETE FROM voice_nodes WHERE id = $1 RETURNING name")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let name = name.ok_or_else(ApiError::not_found)?;
    audit(&mut tx, cu.user.id, "voice_node_delete", id, &name).await?;
    tx.commit().await?;
    state.voice_nodes.forget(NodeId::Db(id));
    state.voice_nodes.invalidate();
    Ok(StatusCode::NO_CONTENT)
}
