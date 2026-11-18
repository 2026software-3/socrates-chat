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

fn unavailable() -> AiError {
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
        let system = || Route {
            ai: self.ai.clone(),
            model: self.config.openai_model.clone(),
        };
        let Some(classroom) = classroom.filter(|_| self.config.classroom_ai_routing) else {
            return Ok(system());
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
            unavailable()
        })?;
        let Some((owner, transitional, disabled, is_teacher)) = row else {
            return Err(unavailable());
        };
        if transitional {
            return Ok(system());
        }
        if disabled || !is_teacher {
            return Err(unavailable());
        }
        let cipher = self
            .config
            .teacher_key_cipher
            .as_deref()
            .ok_or_else(unavailable)?;
        let setting = classroom_ai_store::get(&self.pool, classroom)
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(unavailable)?;
        // 同一場對話已換用備援就從那個位置開始，不跳回主模型
        let start_position: i16 = match conversation {
            Some(id) => {
                sqlx::query_scalar("SELECT ai_fallback_position FROM conversations WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|_| unavailable())?
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
        for (position, m) in models.into_iter().enumerate() {
            let position = position as i16;
            if position < start_position {
                continue;
            }
            // 設定裡的供應商一定在名單內；不在就略過這個目標
            let Some(provider) = ai_providers::find(&m.provider) else {
                continue;
            };
            let mut usable: Vec<UsableKey> = keys::keys_for(&self.pool, cipher, owner, provider.id)
                .await
                .map_err(|_| unavailable())?
                .into_iter()
                .filter(is_eligible)
                .collect();
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
        let first_model = targets.first().ok_or_else(unavailable)?.model.clone();
        let routed = Routed {
            pool: self.pool.clone(),
            classroom,
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
    classroom: Uuid,
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
        if let Err(e) = keys::set_status_after(&self.pool, key, status, retry_after).await {
            tracing::warn!(class = %crate::error::db_error_class(&e), "failed to record key status");
        }
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
    classroom: Uuid,
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
                if let Err(e) = teacher_key_usage::record(&pool, key, classroom, provider, &model, tokens).await {
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
                            self.mark(key, status, e.retry_after()).await;
                            continue;
                        }
                        None => return Err(e),
                    },
                };
                // 串流是惰性的：HTTP 錯誤會是第一個元素。先取第一個元素，才知道這把 key 能不能用。
                match stream.next().await {
                    Some(Err(e)) => match failover_status(&e) {
                        Some(status) => {
                            self.mark(key, status, e.retry_after()).await;
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
        Err(unavailable())
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
                        Some(status) => self.mark(key, status, e.retry_after()).await,
                        None => return Err(e),
                    },
                }
            }
        }
        Err(unavailable())
    }
}
