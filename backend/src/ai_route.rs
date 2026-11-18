//! 教室對話呼叫的 AI 路由（F-40.5，#599；S-03.1「教室 AI 呼叫」）。
//!
//! 所有「為教室內對話內容所做的 AI 呼叫」都先經 `AppState::ai_for_conversation(教室, 對話)`，由這一個函式決定用誰的 key：
//! - 沒有教室（個人練習與全域操作）→ 系統 key（`state.ai`）。
//! - 「過渡中」的教室（NTNU 上線前已存在、教師還沒設定）→ 系統 key，行為不變。
//! - 其他教室 → 只用該教室建立者在該供應商的 key，以教室設定的模型呼叫；**絕不退回系統 key，也絕不用別人的 key**。
//!   建立者帳號停用、不再是教師、沒設定模型、沒有可用 key 時，回 `Failure::ClassroomUnavailable`
//!   （學生看到「教室的 AI 服務暫時無法使用，請聯絡教師」）；復原或重新授予教師身分後自動恢復。
//!
//! 同一供應商的多把 key 輪流使用；遇到失效（401／403）、額度用盡（402、`insufficient_quota`）或限流（429）
//! 就記錄該 key 的狀態與時間，改試下一把。進入非正常狀態的 key 冷卻一段時間後再試（暫定值，待需求方確認）。
//! 只在尚未送出任何內容時換 key（串流的第一個元素就是錯誤）；其餘錯誤照原樣回給呼叫端，沿用 S-03.5 的重試規則。
//! 每次呼叫都記一筆用量（F-40.8）。備援模型切換在 F-40.6。
//!
//! 日誌與診斷事件不含 key 或對話原文。

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicI16, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, stream};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{AiError, AiProvider, AiRequest, AiStream, StructuredCompletion, StructuredRequest},
    ai_providers::{self, Provider},
    classroom_ai_store,
    diagnostics::Failure,
    genai_provider::{AiTarget, GenaiClient, GenaiProvider},
    pii::MaskingAi,
    system_key_store,
    teacher_key_store::{self as keys, KeyStatus, UsableKey},
    teacher_key_usage,
};

/// 進入各狀態後多久不再嘗試該 key。暫定值，待需求方確認。
pub const COOLDOWN_RATE_LIMITED: Duration = Duration::from_secs(60);
pub const COOLDOWN_QUOTA: Duration = Duration::from_secs(600);
pub const COOLDOWN_INVALID: Duration = Duration::from_secs(600);
/// 供應商的 `Retry-After` 最多採信這麼久，避免一個異常值讓 key 長期停用。
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(3600);

/// 解析結果：要用的 provider，以及供診斷記錄的模型名稱。
pub(crate) struct Route {
    pub ai: Arc<dyn AiProvider>,
    pub model: String,
}

/// 教室的 AI 不可用。`reason` 只記在日誌（不含 key 或對話內容），讓維運者知道是哪一項沒設好：
/// `classroom_not_found`、`owner_disabled`、`owner_not_teacher`、`key_encryption_not_configured`、
/// `model_not_configured`、`no_key_for_provider`、`keys_cooling_down`、`all_keys_failed`、`lookup_failed`。
fn unavailable(scope: impl Into<Option<Uuid>>, reason: &'static str) -> AiError {
    // 沒有教室編號的是系統用途的路由
    let classroom = scope.into();
    tracing::warn!(?classroom, reason, "classroom AI unavailable");
    AiError::classified(Failure::ClassroomUnavailable)
}

fn client() -> Result<&'static GenaiClient, AiError> {
    static CLIENT: OnceLock<GenaiClient> = OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c);
    }
    let created = GenaiClient::new()?;
    Ok(CLIENT.get_or_init(|| created))
}

impl AppState {
    /// 這次呼叫該用哪個 AI。`classroom` 是對話所屬的教室（個人練習與全域操作傳 `None`）；
    /// `conversation` 讓路由依對話目前用到的備援位置開始，換用備援後記回對話（同場持續用備援，F-40.6）。
    pub(crate) async fn ai_for_conversation(
        &self,
        classroom: Option<Uuid>,
        conversation: Option<Uuid>,
    ) -> Result<Route, AiError> {
        let Some(classroom) = classroom.filter(|_| self.config.classroom_ai_routing) else {
            return Ok(self.system_route().await);
        };
        let row: Option<(Uuid, bool, bool, bool)> = sqlx::query_as(
            "SELECT c.created_by, c.ai_transitional, u.disabled_at IS NOT NULL,
                    EXISTS (SELECT 1 FROM teachers t WHERE t.email = u.email)
               FROM classrooms c JOIN users u ON u.id = c.created_by
              WHERE c.id = $1",
        )
        .bind(classroom)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            tracing::error!(class = %crate::error::db_error_class(&e), "classroom AI route lookup failed");
            unavailable(classroom, "lookup_failed")
        })?;
        let Some((owner, transitional, disabled, is_teacher)) = row else {
            return Err(unavailable(classroom, "classroom_not_found"));
        };
        if transitional {
            return Ok(self.system_route().await);
        }
        if disabled {
            return Err(unavailable(classroom, "owner_disabled"));
        }
        if !is_teacher {
            return Err(unavailable(classroom, "owner_not_teacher"));
        }
        let cipher = self
            .config
            .teacher_key_cipher
            .as_deref()
            .ok_or_else(|| unavailable(classroom, "key_encryption_not_configured"))?;
        let setting = classroom_ai_store::get(&self.pool, classroom)
            .await
            .map_err(|_| unavailable(classroom, "lookup_failed"))?
            .ok_or_else(|| unavailable(classroom, "model_not_configured"))?;
        // 同一場對話已換用備援就從那個位置開始，不跳回主模型
        let start_position: i16 = match conversation {
            Some(id) => {
                sqlx::query_scalar("SELECT ai_fallback_position FROM conversations WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|_| unavailable(classroom, "lookup_failed"))?
                    .unwrap_or(0)
            }
            None => 0,
        };
        let mut models = vec![setting.primary.clone()];
        models.extend(setting.fallbacks.iter().cloned());
        // 記下的位置超出目前清單（不應發生，設定變更時會歸零）：從主模型開始，不讓對話卡住
        let start_position = if usize::try_from(start_position).is_ok_and(|p| p < models.len()) {
            start_position
        } else {
            0
        };
        let mut targets = Vec::new();
        // 有 key 但都在冷卻中，和根本沒有該供應商的 key，在日誌裡分開記
        let mut any_key = false;
        for (position, m) in models.into_iter().enumerate() {
            let position = position as i16;
            if position < start_position {
                continue;
            }
            // 設定裡的供應商一定在名單內；不在就略過這個目標
            let Some(provider) = ai_providers::find(&m.provider) else {
                continue;
            };
            let all = keys::keys_for(&self.pool, cipher, owner, provider.id)
                .await
                .map_err(|_| unavailable(classroom, "lookup_failed"))?;
            any_key |= !all.is_empty();
            let mut usable: Vec<UsableKey> = all.into_iter().filter(is_eligible).collect();
            if usable.is_empty() {
                continue;
            }
            // 輪流：每次呼叫從下一把開始
            let start = self.key_rotation.fetch_add(1, Ordering::Relaxed) % usable.len();
            usable.rotate_left(start);
            targets.push(Target {
                position,
                provider,
                model: m.model,
                keys: usable,
            });
        }
        let first_model = targets
            .first()
            .ok_or_else(|| {
                unavailable(
                    classroom,
                    if any_key {
                        "keys_cooling_down"
                    } else {
                        "no_key_for_provider"
                    },
                )
            })?
            .model
            .clone();
        let routed = Routed {
            pool: self.pool.clone(),
            classroom: Some(classroom),
            conversation,
            endpoint: self.route_endpoint.clone(),
            targets,
            client: client()?,
            failed: Mutex::default(),
            settled: AtomicI16::new(0),
        };
        Ok(Route {
            ai: Arc::new(MaskingAi::new(Arc::new(routed))),
            model: first_model,
        })
    }

    /// 系統用途的 AI：沒有教室的呼叫、過渡中的教室、翻譯、主張分組、維度與公告翻譯等。
    /// 管理者在介面設定了供應商、模型與系統 key 就用它們（多把輪流、失效或限流自動換下一把）；
    /// 沒有設定、沒有可用的 key 或全部失敗時，退回 `self.ai`；它的主機 key 池已淘汰、永遠是空的，所以會失敗（AI 無法使用），不會連到任何服務。
    pub(crate) async fn system_route(&self) -> Route {
        match self.configured_system_route().await {
            Some(route) => route,
            None => Route {
                ai: self.ai.clone(),
                model: self.config.openai_model.clone(),
            },
        }
    }

    async fn configured_system_route(&self) -> Option<Route> {
        if !self.config.classroom_ai_routing {
            return None;
        }
        let cipher = self.config.teacher_key_cipher.as_deref()?;
        let setting = system_key_store::get_setting(&self.pool).await.ok()??;
        let provider = ai_providers::find(&setting.provider)?;
        let mut usable: Vec<UsableKey> =
            system_key_store::keys_for(&self.pool, cipher, provider.id)
                .await
                .ok()?
                .into_iter()
                .filter(is_eligible)
                .collect();
        if usable.is_empty() {
            return None;
        }
        let start = self.key_rotation.fetch_add(1, Ordering::Relaxed) % usable.len();
        usable.rotate_left(start);
        let model = setting.model;
        let routed = Routed {
            pool: self.pool.clone(),
            classroom: None,
            conversation: None,
            endpoint: self.route_endpoint.clone(),
            targets: vec![Target {
                position: 0,
                provider,
                model: model.clone(),
                keys: usable,
            }],
            client: client().ok()?,
            failed: Mutex::default(),
            settled: AtomicI16::new(0),
        };
        Some(Route {
            ai: Arc::new(WithFallback {
                primary: Arc::new(MaskingAi::new(Arc::new(routed))),
                fallback: self.ai.clone(),
            }),
            model,
        })
    }
}

/// 系統用途：管理者設定的 key 都不能用時，退回 `self.ai`（沒有 key，會直接失敗）。
struct WithFallback {
    primary: Arc<dyn AiProvider>,
    fallback: Arc<dyn AiProvider>,
}

#[async_trait]
impl AiProvider for WithFallback {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        match self.primary.stream_chat(req.clone()).await {
            Err(e) if e.class == Failure::ClassroomUnavailable => {
                tracing::warn!("system AI keys unavailable; using the host key list");
                self.fallback.stream_chat(req).await
            }
            other => other,
        }
    }

    async fn complete_json(&self, req: StructuredRequest) -> Result<StructuredCompletion, AiError> {
        match self.primary.complete_json(req.clone()).await {
            Err(e) if e.class == Failure::ClassroomUnavailable => {
                tracing::warn!("system AI keys unavailable; using the host key list");
                self.fallback.complete_json(req).await
            }
            other => other,
        }
    }
}

/// 正常的 key，或冷卻已過的非正常 key（再給一次機會）。
fn is_eligible(key: &UsableKey) -> bool {
    let cooldown = match key.status {
        KeyStatus::Ok => return true,
        // 供應商要求等更久就照它的（最多一小時），否則固定冷卻
        KeyStatus::RateLimited => key
            .retry_after
            .map_or(COOLDOWN_RATE_LIMITED, |d| d.min(MAX_RETRY_AFTER))
            .max(COOLDOWN_RATE_LIMITED),
        KeyStatus::QuotaExhausted => COOLDOWN_QUOTA,
        KeyStatus::Invalid => COOLDOWN_INVALID,
    };
    match key.status_at {
        Some(at) => (Utc::now() - at)
            .to_std()
            .map(|d| d >= cooldown)
            .unwrap_or(false),
        None => true,
    }
}

/// 會讓我們改試下一把 key 的失敗，以及該記成什麼狀態。
fn failover_status(error: &AiError) -> Option<KeyStatus> {
    match error.class {
        Failure::InvalidKey => Some(KeyStatus::Invalid),
        Failure::QuotaExceeded => Some(KeyStatus::QuotaExhausted),
        Failure::RateLimited => Some(KeyStatus::RateLimited),
        _ => None,
    }
}

/// 一個候選模型（主模型或第 n 個備援）與教師在該供應商可用的 key（依輪流順序）。
struct Target {
    /// 0 是主模型，1–3 是備援
    position: i16,
    provider: &'static Provider,
    model: String,
    keys: Vec<UsableKey>,
}

struct Routed {
    pool: PgPool,
    /// 對話所屬的教室；`None` 是系統用途（用系統 key，用量記在系統用量）
    classroom: Option<Uuid>,
    conversation: Option<Uuid>,
    /// 測試用：把所有供應商端點換成假服務
    endpoint: Option<String>,
    targets: Vec<Target>,
    client: &'static GenaiClient,
    /// 這次解析中已確定不能用的 key；同一次回覆的重試不再打擾它們
    failed: Mutex<HashSet<Uuid>>,
    /// 已成功換用的備援位置；之後的呼叫從這裡開始
    settled: AtomicI16,
}

impl Routed {
    fn provider_for(&self, target: &Target, key: &UsableKey) -> Arc<GenaiProvider> {
        Arc::new(
            self.client.provider(AiTarget::new(
                target.provider.adapter,
                self.endpoint
                    .clone()
                    .unwrap_or_else(|| target.provider.endpoint.to_owned()),
                target.model.clone(),
                key.secret(),
            )),
        )
    }

    fn guard(&self, target: &Target, key: &UsableKey, gp: &Arc<GenaiProvider>) -> UsageGuard {
        UsageGuard {
            pool: self.pool.clone(),
            key_id: key.id,
            classroom: self.classroom,
            provider: target.provider.id,
            model: target.model.clone(),
            gp: gp.clone(),
        }
    }

    /// 這個目標或 key 在這次解析中是否還不值得試。
    fn skip(&self, target: &Target, key: Option<&UsableKey>) -> bool {
        target.position < self.settled.load(Ordering::Relaxed)
            || key.is_some_and(|k| self.failed.lock().is_ok_and(|f| f.contains(&k.id)))
    }

    /// 記錄這把 key 的狀態（含供應商要求的等待時間）。只寫到同一把 key 上，
    /// 呼叫期間教師更換了 key 時不覆寫新 key 的狀態。失敗的 key 在這次解析中不再試；
    /// 恢復成 `Ok` 的 key 照常可用（之後的重試仍可用它）。
    async fn mark(&self, key: &UsableKey, status: KeyStatus, retry_after: Option<Duration>) {
        if status != KeyStatus::Ok
            && let Ok(mut failed) = self.failed.lock()
        {
            failed.insert(key.id);
        }
        let recorded = if self.classroom.is_some() {
            keys::set_status_after(&self.pool, key, status, retry_after).await
        } else {
            system_key_store::set_status_after(&self.pool, key, status, retry_after).await
        };
        if let Err(e) = recorded {
            tracing::warn!(class = %crate::error::db_error_class(&e), "failed to record key status");
        }
    }

    /// 這把 key 因失效、額度或限流不能用：記在日誌（只有 key 的識別碼與失敗分類），記錄狀態後換下一把。
    async fn fail_over(
        &self,
        target: &Target,
        key: &UsableKey,
        error: &AiError,
        status: KeyStatus,
    ) {
        tracing::warn!(
            classroom = ?self.classroom,
            key_id = %key.id,
            provider = target.provider.id,
            model = %target.model,
            class = error.class.code(),
            status = status.as_str(),
            "API key failed; trying the next"
        );
        self.mark(key, status, error.retry_after()).await;
    }

    /// 這把 key 成功了：之前非正常的狀態恢復。
    async fn recovered(&self, key: &UsableKey) {
        if key.status != KeyStatus::Ok {
            self.mark(key, KeyStatus::Ok, None).await;
        }
    }

    /// 這個目標成功了：若是備援，記回對話，之後同場都從這裡開始。
    async fn settled_on(&self, target: &Target) {
        self.settled.fetch_max(target.position, Ordering::Relaxed);
        if target.position == 0 {
            return;
        }
        let Some(conversation) = self.conversation else {
            return;
        };
        if let Err(e) = sqlx::query(
            "UPDATE conversations SET ai_fallback_position = $2
              WHERE id = $1 AND ai_fallback_position < $2",
        )
        .bind(conversation)
        .bind(target.position)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(class = %crate::error::db_error_class(&e), "failed to record fallback position");
        }
    }
}

/// 一次呼叫結束（含串流中途被丟棄）時記一筆用量；供應商沒回報 token 時只計次數。
struct UsageGuard {
    pool: PgPool,
    key_id: Uuid,
    /// `None` 是系統用途
    classroom: Option<Uuid>,
    provider: &'static str,
    model: String,
    gp: Arc<GenaiProvider>,
}

impl Drop for UsageGuard {
    fn drop(&mut self) {
        let usage = self.gp.last_usage();
        let tokens = match (usage.input, usage.output) {
            (Some(i), Some(o)) => Some((
                i64::try_from(i).unwrap_or(i64::MAX),
                i64::try_from(o).unwrap_or(i64::MAX),
            )),
            _ => None,
        };
        let (pool, key, classroom, provider, model) = (
            self.pool.clone(),
            self.key_id,
            self.classroom,
            self.provider,
            std::mem::take(&mut self.model),
        );
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let recorded = match classroom {
                    Some(classroom) => {
                        teacher_key_usage::record(&pool, key, classroom, provider, &model, tokens).await
                    }
                    None => system_key_store::record_usage(&pool, key, provider, &model, tokens).await,
                };
                if let Err(e) = recorded {
                    tracing::warn!(class = %crate::error::db_error_class(&e), "failed to record AI usage");
                }
            });
        }
    }
}

#[async_trait]
impl AiProvider for Routed {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        // 先試主模型的每把 key，全部因限流、額度用盡或失效不能用，才換下一個備援（F-40.6）；
        // 只在尚未送出任何內容時換，其餘錯誤照原樣回給呼叫端。
        for target in &self.targets {
            if self.skip(target, None) {
                continue;
            }
            for key in &target.keys {
                if self.skip(target, Some(key)) {
                    continue;
                }
                let gp = self.provider_for(target, key);
                let guard = self.guard(target, key, &gp);
                let mut stream = match gp.stream_chat(req.clone()).await {
                    Ok(s) => s,
                    Err(e) => match failover_status(&e) {
                        Some(status) => {
                            self.fail_over(target, key, &e, status).await;
                            continue;
                        }
                        None => return Err(e),
                    },
                };
                // 串流是惰性的：HTTP 錯誤會是第一個元素。先取第一個元素，才知道這把 key 能不能用。
                match stream.next().await {
                    Some(Err(e)) => match failover_status(&e) {
                        Some(status) => {
                            self.fail_over(target, key, &e, status).await;
                            continue;
                        }
                        None => return Err(e),
                    },
                    Some(Ok(first)) => {
                        self.recovered(key).await;
                        self.settled_on(target).await;
                        let rest = stream::once(async move { Ok(first) }).chain(stream);
                        // guard 跟著串流：串流結束或被丟棄時記用量
                        return Ok(rest
                            .chain(
                                stream::once(async move {
                                    drop(guard);
                                    None
                                })
                                .filter_map(|x: Option<Result<String, AiError>>| async move { x }),
                            )
                            .boxed());
                    }
                    None => {
                        self.recovered(key).await;
                        self.settled_on(target).await;
                        return Ok(stream::empty().boxed());
                    }
                }
            }
        }
        Err(unavailable(self.classroom, "all_keys_failed"))
    }

    async fn complete_json(&self, req: StructuredRequest) -> Result<StructuredCompletion, AiError> {
        for target in &self.targets {
            if self.skip(target, None) {
                continue;
            }
            for key in &target.keys {
                if self.skip(target, Some(key)) {
                    continue;
                }
                let gp = self.provider_for(target, key);
                let _guard = self.guard(target, key, &gp);
                match gp.complete_json(req.clone()).await {
                    Ok(done) => {
                        self.recovered(key).await;
                        self.settled_on(target).await;
                        return Ok(done);
                    }
                    Err(e) => match failover_status(&e) {
                        Some(status) => self.fail_over(target, key, &e, status).await,
                        None => return Err(e),
                    },
                }
            }
        }
        Err(unavailable(self.classroom, "all_keys_failed"))
    }
}
