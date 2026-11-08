//! PostgreSQL-backed administration diagnostics (F-33.1 / S-09.3).

use std::{
    future::Future,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    http::{HeaderValue, StatusCode, header},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireAdmin,
    error::{ApiError, ErrorBody},
    pagination::{self, Page, Pagination},
};

const WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const PAGE_RESOURCE: &str = "admin_diagnostics";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/status", get(status))
        .route("/api/admin/diagnostics", get(list))
        .route_layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            },
        ))
}

#[derive(Clone)]
struct RequestContext {
    id: Uuid,
    route: String,
}

tokio::task_local! {
    static CONTEXT: RequestContext;
}

pub(crate) fn scope_request<F: Future>(
    id: Uuid,
    route: &str,
    future: F,
) -> impl Future<Output = F::Output> {
    CONTEXT.scope(
        RequestContext {
            id,
            route: route.to_owned(),
        },
        future,
    )
}

/// Tokio tasks do not inherit task-local state. Capture the initiating request
/// before spawning a reply or summary, without copying any request data.
pub(crate) fn inherit_request<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let context = CONTEXT.try_with(Clone::clone).ok();
    async move {
        match context {
            Some(context) => CONTEXT.scope(context, future).await,
            None => future.await,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    ConnectionFailed,
    Timeout,
    InvalidKey,
    QuotaExceeded,
    RateLimited,
    ServiceError,
    InvalidCredentials,
    AccountDisabled,
    AccountLocked,
}

impl Failure {
    fn code(self) -> &'static str {
        match self {
            Self::ConnectionFailed => "connection_failed",
            Self::Timeout => "timeout",
            Self::InvalidKey => "invalid_key",
            Self::QuotaExceeded => "quota_exceeded",
            Self::RateLimited => "rate_limited",
            Self::ServiceError => "service_error",
            Self::InvalidCredentials => "invalid_credentials",
            Self::AccountDisabled => "account_disabled",
            Self::AccountLocked => "account_locked",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Ai,
    Login,
    AccountLock,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::Login => "login",
            Self::AccountLock => "account_lock",
        }
    }
}

struct Event<'a> {
    kind: Kind,
    failure: Option<Failure>,
    function: &'static str,
    status: u16,
    error_code: Option<&'static str>,
    user: Option<Uuid>,
    classroom: Option<Uuid>,
    ai: Option<AiDetails<'a>>,
}

struct AiDetails<'a> {
    duration_ms: i64,
    retries: i32,
    model: Option<&'a str>,
}

async fn insert(pool: &PgPool, event: Event<'_>) {
    let context = CONTEXT
        .try_with(Clone::clone)
        .unwrap_or_else(|_| RequestContext {
            id: Uuid::new_v4(),
            route: "<background>".into(),
        });
    let model = event.ai.as_ref().and_then(|ai| ai.model).filter(|model| {
        !model.is_empty()
            && model.len() <= 128
            && !model.starts_with("sk-")
            && !model.starts_with("AIza")
            && model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b))
    });
    let write = sqlx::query(
        "INSERT INTO diagnostic_events
            (request_id, kind, failure_class, function, route, status, error_code,
             user_id, classroom_id, duration_ms, retry_count, model)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(context.id)
    .bind(event.kind.code())
    .bind(event.failure.map(Failure::code))
    .bind(event.function)
    .bind(&context.route)
    .bind(event.status as i16)
    .bind(event.error_code)
    .bind(event.user)
    .bind(event.classroom)
    .bind(event.ai.as_ref().map(|ai| ai.duration_ms))
    .bind(event.ai.as_ref().map(|ai| ai.retries))
    .bind(model)
    .execute(pool);
    match tokio::time::timeout(WRITE_TIMEOUT, write).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(class = %crate::error::db_error_class(&error), "diagnostic write failed")
        }
        Err(_) => tracing::warn!(class = "timeout", "diagnostic write failed"),
    }
}

pub(crate) async fn login(
    pool: &PgPool,
    function: &'static str,
    user: Option<Uuid>,
    outcome: Result<(), (Failure, ApiError)>,
) {
    let (failure, status, error_code) = match outcome {
        Ok(()) => (
            None,
            if function == "google_login" { 303 } else { 200 },
            None,
        ),
        Err((failure, error)) => (Some(failure), error.status.as_u16(), Some(error.code)),
    };
    insert(
        pool,
        Event {
            kind: Kind::Login,
            failure,
            function,
            status,
            error_code,
            user,
            classroom: None,
            ai: None,
        },
    )
    .await;
}

pub(crate) async fn account_lock(pool: &PgPool, user: Option<Uuid>, response: &ApiError) {
    insert(
        pool,
        Event {
            kind: Kind::AccountLock,
            failure: Some(Failure::AccountLocked),
            function: "password_login",
            status: response.status.as_u16(),
            error_code: Some(response.code),
            user,
            classroom: None,
            ai: None,
        },
    )
    .await;
}

#[derive(Clone, Copy)]
pub(crate) struct AiActivity {
    function: &'static str,
    user: Option<Uuid>,
    classroom: Option<Uuid>,
    success_status: u16,
    failure_status: u16,
    failure_code: &'static str,
}

impl AiActivity {
    pub(crate) fn new(function: &'static str, user: Option<Uuid>, classroom: Option<Uuid>) -> Self {
        Self {
            function,
            user,
            classroom,
            success_status: 200,
            failure_status: 502,
            failure_code: "ai_unavailable",
        }
    }

    pub(crate) fn statuses(mut self, success: u16, failure: u16) -> Self {
        self.success_status = success;
        self.failure_status = failure;
        self
    }

    pub(crate) fn failure_code(mut self, code: &'static str) -> Self {
        self.failure_code = code;
        self
    }

    pub(crate) async fn run<T>(
        self,
        state: &AppState,
        model: Option<&str>,
        retry: i32,
        future: impl Future<Output = Result<T, crate::ai::AiError>>,
    ) -> Result<T, crate::ai::AiError> {
        let started = Instant::now();
        let result = future.await;
        self.record(state, model, retry, started, result.as_ref().err())
            .await;
        result
    }

    pub(crate) async fn record(
        self,
        state: &AppState,
        model: Option<&str>,
        retry: i32,
        started: Instant,
        error: Option<&crate::ai::AiError>,
    ) {
        let ai = AiDetails {
            duration_ms: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
            retries: retry + error.map_or(0, |error| (error.provider_attempts - 1).max(0)),
            model: error.and_then(|error| error.model.as_deref()).or(model),
        };
        insert(
            &state.pool,
            Event {
                kind: Kind::Ai,
                failure: error.map(|error| error.class),
                function: self.function,
                status: if error.is_some() {
                    self.failure_status
                } else {
                    self.success_status
                },
                error_code: error.map(|_| self.failure_code),
                user: self.user,
                classroom: self.classroom,
                ai: Some(ai),
            },
        )
        .await;
    }
}

#[derive(Default, Clone, Copy, Deserialize, Serialize, ToSchema)]
enum Period {
    #[default]
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
    #[serde(rename = "30d")]
    Days30,
}

impl Period {
    fn hours(self) -> i32 {
        match self {
            Self::Hours24 => 24,
            Self::Days7 => 168,
            Self::Days30 => 720,
        }
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct StatusQuery {
    #[serde(default)]
    period: Period,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum AiStatus {
    NoData,
    Ok,
    HasFailures,
}

#[derive(FromRow)]
struct Counts {
    ai_calls: i64,
    ai_failures: i64,
    login_failures: i64,
    account_locks: i64,
}

#[derive(Serialize, ToSchema)]
struct Statistics {
    ai_status: AiStatus,
    ai_calls: i64,
    ai_failures: i64,
    login_failures: i64,
    account_locks: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SystemStatus {
    service: &'static str,
    database: &'static str,
    period: Period,
    diagnostics_available: bool,
    statistics: Option<Statistics>,
    recent_errors: Vec<DiagnosticEvent>,
}

#[derive(FromRow, Serialize, ToSchema)]
pub(crate) struct DiagnosticEvent {
    #[serde(skip)]
    id: i64,
    occurred_at: DateTime<Utc>,
    request_id: Uuid,
    kind: String,
    failure_class: String,
    function: String,
    route: String,
    status: i16,
    error_code: String,
    user_id: Option<Uuid>,
    classroom_id: Option<Uuid>,
    duration_ms: Option<i64>,
    retry_count: Option<i32>,
    model: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
struct Filters {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    kind: Option<Kind>,
    failure_class: Option<Failure>,
    request_id: Option<Uuid>,
    user_id: Option<Uuid>,
    classroom_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    #[serde(flatten)]
    filters: Filters,
    #[serde(flatten)]
    pagination: Pagination,
}

#[derive(Deserialize, Serialize)]
struct Cursor {
    time: DateTime<Utc>,
    id: i64,
    filter: String,
}

#[utoipa::path(get, path = "/api/admin/status", tag = "admin",
    params(StatusQuery),
    responses((status = 200, body = SystemStatus), (status = 400, body = ErrorBody),
              (status = 401, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub(crate) async fn status(
    _: RequireAdmin,
    State(state): State<AppState>,
    query: Result<Query<StatusQuery>, QueryRejection>,
) -> Result<Json<SystemStatus>, ApiError> {
    let Query(query) = query.map_err(|_| invalid_filter())?;
    let db_ok = matches!(
        tokio::time::timeout(
            crate::READY_DB_TIMEOUT,
            sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool),
        )
        .await,
        Ok(Ok(_))
    );
    let mut view = SystemStatus {
        service: "ok",
        database: if db_ok { "ok" } else { "unavailable" },
        period: query.period,
        diagnostics_available: false,
        statistics: None,
        recent_errors: Vec::new(),
    };
    if !db_ok {
        return Ok(Json(view));
    }
    let load = async {
        let from = Utc::now() - chrono::Duration::hours(i64::from(query.period.hours()));
        let counts = sqlx::query_as::<_, Counts>(
            "SELECT count(*) FILTER (WHERE kind = 'ai') AS ai_calls,
                    count(*) FILTER (WHERE kind = 'ai' AND failure_class IS NOT NULL) AS ai_failures,
                    count(*) FILTER (WHERE kind = 'login' AND failure_class IS NOT NULL) AS login_failures,
                    count(*) FILTER (WHERE kind = 'account_lock') AS account_locks
             FROM diagnostic_events WHERE occurred_at >= $1 AND occurred_at <= now()
                AND occurred_at >= now() - interval '720 hours'",
        ).bind(from).fetch_one(&state.pool);
        let filters = Filters {
            from: Some(from),
            ..Filters::default()
        };
        tokio::try_join!(counts, find(&state.pool, &filters, None, 10))
    };
    match tokio::time::timeout(crate::READY_DB_TIMEOUT, load).await {
        Ok(Ok((counts, errors))) => {
            view.diagnostics_available = true;
            view.recent_errors = errors;
            let ai_status = if counts.ai_failures > 0 {
                AiStatus::HasFailures
            } else if counts.ai_calls > 0 {
                AiStatus::Ok
            } else {
                AiStatus::NoData
            };
            view.statistics = Some(Statistics {
                ai_status,
                ai_calls: counts.ai_calls,
                ai_failures: counts.ai_failures,
                login_failures: counts.login_failures,
                account_locks: counts.account_locks,
            });
        }
        Ok(Err(error)) => {
            tracing::warn!(class = %crate::error::db_error_class(&error), "diagnostic read failed")
        }
        Err(_) => tracing::warn!(class = "timeout", "diagnostic read failed"),
    }
    Ok(Json(view))
}

#[utoipa::path(get, path = "/api/admin/diagnostics", tag = "admin",
    params(("from" = Option<DateTime<Utc>>, Query), ("to" = Option<DateTime<Utc>>, Query),
        ("kind" = Option<Kind>, Query), ("failure_class" = Option<Failure>, Query),
        ("request_id" = Option<Uuid>, Query), ("user_id" = Option<Uuid>, Query), ("classroom_id" = Option<Uuid>, Query),
        ("limit" = Option<usize>, Query, description = "1–200，預設 50"),
        ("cursor" = Option<String>, Query, description = "前一頁的 next_cursor")),
    responses((status = 200, body = Page<DiagnosticEvent>), (status = 400, body = ErrorBody),
              (status = 401, body = ErrorBody), (status = 403, body = ErrorBody), (status = 503, body = ErrorBody)))]
pub(crate) async fn list(
    _: RequireAdmin,
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Page<DiagnosticEvent>>, ApiError> {
    let Query(query) = query.map_err(|_| invalid_filter())?;
    let filters = query.filters;
    if filters
        .from
        .zip(filters.to)
        .is_some_and(|(from, to)| from >= to)
    {
        return Err(invalid_filter());
    }
    let limit = query.pagination.limit()?;
    let fingerprint = pagination::fingerprint(&filters);
    let cursor = query.pagination.cursor::<Cursor>(PAGE_RESOURCE)?;
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.filter != fingerprint)
    {
        return Err(ApiError::bad_request("invalid_cursor", "分頁游標不正確"));
    }
    let rows = match tokio::time::timeout(
        crate::READY_DB_TIMEOUT,
        find(&state.pool, &filters, cursor.as_ref(), (limit + 1) as i64),
    )
    .await
    {
        Ok(Ok(rows)) => rows,
        Ok(Err(error)) => {
            tracing::warn!(class = %crate::error::db_error_class(&error), "diagnostic read failed");
            return Err(diagnostics_unavailable());
        }
        Err(_) => {
            tracing::warn!(class = "timeout", "diagnostic read failed");
            return Err(diagnostics_unavailable());
        }
    };
    pagination::page(rows, limit, PAGE_RESOURCE, |last| Cursor {
        time: last.occurred_at,
        id: last.id,
        filter: fingerprint,
    })
    .map(Json)
}

async fn find(
    pool: &PgPool,
    filters: &Filters,
    cursor: Option<&Cursor>,
    limit: i64,
) -> Result<Vec<DiagnosticEvent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, occurred_at, request_id, kind, failure_class, function, route, status, error_code,
                user_id, classroom_id, duration_ms, retry_count, model
         FROM diagnostic_events
         WHERE failure_class IS NOT NULL AND occurred_at >= now() - interval '720 hours' AND occurred_at <= now()
            AND ($1::timestamptz IS NULL OR occurred_at >= $1)
            AND ($2::timestamptz IS NULL OR occurred_at < $2)
            AND ($3::text IS NULL OR kind = $3) AND ($4::text IS NULL OR failure_class = $4)
            AND ($5::uuid IS NULL OR request_id = $5) AND ($6::uuid IS NULL OR user_id = $6)
            AND ($7::uuid IS NULL OR classroom_id = $7)
            AND ($8::timestamptz IS NULL OR (occurred_at, id) < ($8, $9::bigint))
         ORDER BY occurred_at DESC, id DESC LIMIT $10",
    )
    .bind(filters.from).bind(filters.to).bind(filters.kind.map(Kind::code))
    .bind(filters.failure_class.map(Failure::code)).bind(filters.request_id)
    .bind(filters.user_id).bind(filters.classroom_id)
    .bind(cursor.map(|cursor| cursor.time)).bind(cursor.map(|cursor| cursor.id))
    .bind(limit).fetch_all(pool).await
}

fn invalid_filter() -> ApiError {
    ApiError::bad_request("invalid_filter", "篩選條件不正確")
}

fn diagnostics_unavailable() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "diagnostics_unavailable",
        "診斷資料暫時無法讀取",
    )
}

pub async fn purge_expired(pool: &PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query("DELETE FROM diagnostic_events WHERE occurred_at < now() - interval '720 hours'")
        .execute(pool)
        .await
        .map(|result| result.rows_affected())
}

pub fn spawn_retention(pool: PgPool) {
    tokio::spawn(async move {
        let mut daily = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        daily.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            daily.tick().await;
            if let Err(error) = purge_expired(&pool).await {
                tracing::warn!(class = %crate::error::db_error_class(&error), "diagnostic retention cleanup failed");
            }
        }
    });
}
