//! AI 服務介面（S-03.1、S-08.1）：所有 AI 呼叫都經過 `AiProvider`，測試時注入假實作。
//!
//! 本檔另含蘇格拉底式追問的提示（S-03.2）與每回合結構化結果的解析。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::key_pool::{KeyPool, parse_retry_after};

/// 提示與規則的版本；記錄在每則 AI 回覆上（S-03.2），修改提示時一併更新。
/// 送出前的個資遮蔽規則（`pii`，S-02.6）也併入這個版本，遮蔽規則變動時同樣遞增。
/// v1.11：加入個資遮蔽與系統提示的佔位詞說明。
pub const RULES_VERSION: &str = "v1.11/zh-TW";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

impl ChatRole {
    /// 資料庫 `messages.role`（`student`／`ai`）對應的角色。
    pub fn from_db_role(role: &str) -> Self {
        if role == "student" {
            Self::User
        } else {
            Self::Assistant
        }
    }
}

/// 取出模型輸出中第一個 `{` 到最後一個 `}` 之間的 JSON；找不到或格式不符回傳 `None`。
fn extract_json<T: DeserializeOwned>(raw: &str) -> Option<T> {
    let (start, end) = (raw.find('{')?, raw.rfind('}')?);
    if end < start {
        return None;
    }
    serde_json::from_str(&raw[start..=end]).ok()
}

/// 去掉前後空白並截到 `max` 個字元；結果為空時回傳 `None`。
fn clip(s: &str, max: usize) -> Option<String> {
    let s: String = s.trim().chars().take(max).collect();
    (!s.is_empty()).then_some(s)
}

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AiRequest {
    pub system: String,
    pub messages: Vec<ChatTurn>,
}

/// 外部 AI 服務錯誤。不帶內部細節，避免被帶到回應或日誌（S-08.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiError;

/// 依序給出文字片段。空字串代表模型開始推理、還沒有正文：算作首個 token（S-03.5），但不帶文字。
pub type AiStream = BoxStream<'static, Result<String, AiError>>;

#[async_trait]
pub trait AiProvider: Send + Sync {
    /// 開始串流回覆；回傳的串流依序給出文字片段（見 `AiStream`）。
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError>;
}

// ---- 每回合的結構化結果 ----

/// 文字回覆與結構化結果之間的分隔標記。
pub const META_DELIM: &str = "<<<META>>>";

/// 一次回覆分成「給學生看的文字」與「結構化結果」兩段；串流時文字立即送出，
/// 分隔標記之後的內容留在後端解析。
#[derive(Default)]
pub struct MetaSplitter {
    held: String,
    in_meta: bool,
    meta: String,
}

impl MetaSplitter {
    /// 收進一段片段，回傳可以立即送給學生的文字。
    pub fn push(&mut self, chunk: &str) -> String {
        if self.in_meta {
            self.meta.push_str(chunk);
            return String::new();
        }
        self.held.push_str(chunk);
        if let Some(i) = self.held.find(META_DELIM) {
            self.meta = self.held[i + META_DELIM.len()..].to_string();
            self.in_meta = true;
            let text = self.held[..i].to_string();
            self.held.clear();
            return text;
        }
        // 暫存結尾可能是分隔標記開頭的部分，等下一段再判斷
        let keep = (1..META_DELIM.len())
            .rev()
            .find(|&n| {
                n <= self.held.len()
                    && self.held.is_char_boundary(self.held.len() - n)
                    && META_DELIM.starts_with(&self.held[self.held.len() - n..])
            })
            .unwrap_or(0);
        // 送出前段、留下後段：沿用原本的配置，不另外複製
        let rest = self.held.split_off(self.held.len() - keep);
        std::mem::replace(&mut self.held, rest)
    }

    /// 串流結束：回傳尚未送出的文字與結構化段落。
    pub fn finish(self) -> (String, String) {
        if self.in_meta {
            (String::new(), self.meta)
        } else {
            (self.held, String::new())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnMeta {
    pub question_type: Option<String>,
    pub advance: bool,
    /// 模型對階段判斷的理由（S-03.2）；只供稽核與評估，不回傳給學生。
    pub reason: Option<String>,
}

/// 判斷理由的長度上限（字元）。
const MAX_REASON_CHARS: usize = 500;

const QUESTION_TYPES: [&str; 6] = [
    "clarify",
    "reason",
    "assumption",
    "counterexample",
    "perspective",
    "wrap_up",
];

/// 解析結構化段落；格式不符時保守地當作「不晉級、未知類型」。
pub fn parse_meta(raw: &str) -> TurnMeta {
    #[derive(Deserialize)]
    struct Raw {
        question_type: Option<String>,
        #[serde(default)]
        advance: bool,
        reason: Option<String>,
    }
    match extract_json::<Raw>(raw) {
        Some(r) => TurnMeta {
            question_type: r
                .question_type
                .filter(|q| QUESTION_TYPES.contains(&q.as_str())),
            advance: r.advance,
            reason: r.reason.and_then(|s| clip(&s, MAX_REASON_CHARS)),
        },
        None => TurnMeta {
            question_type: None,
            advance: false,
            reason: None,
        },
    }
}

// ---- 提示（S-03.2、S-03.3）----

pub struct PromptContext<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub stage: i16,
    pub turn: i32,
    pub wrap_up_turn: u32,
}

pub fn build_system_prompt(c: &PromptContext) -> String {
    let stage = match c.stage {
        1 => {
            "第 1 階段「釐清」：讓學生說清楚立場，定義關鍵詞、確認立場。學生已表達明確立場並說明關鍵詞的意思時，才算可進入下一階段。"
        }
        2 => {
            "第 2 階段「論證」：檢視立場背後的理由與隱含假設。學生至少提出一個理由且檢視過一個隱含假設時，才算可進入下一階段。"
        }
        _ => {
            "第 3 階段「挑戰」：用反例或對立觀點測試立場的韌性。學生已回應至少一個反例或對立觀點時，才算達成。"
        }
    };
    let wrap = if c.turn >= c.wrap_up_turn as i32 {
        "\n目前討論已進行很多回合：請開始引導學生整理自己的結論，並提議收尾。"
    } else {
        ""
    };
    format!(
        "你是一位蘇格拉底式的哲學討論引導者，使用繁體中文與學生對話。\n\
         討論題目：{title}\n題目說明：{desc}\n\n\
        規則：\n\
        - 每次回覆只問一個問題，以追問引導學生思考。\n\
        - 不替學生下結論，不表達自己的立場。\n\
        - 語氣中立、尊重，回覆簡短；離題時引導回主題。\n\
        - 方括號大寫詞（[EMAIL]、[PHONE]、[NATIONAL_ID]、[STUDENT_ID]）是系統遮蔽掉的個人資料：不要追問或猜測內容，也不要照抄，繼續以題目為主討論。\n\n\
        追問與分類：\n\
        先依對話提出一個有幫助的追問，再根據實際追問標記 question_type。\n\
        每次只要求一個主要思考操作，避免先問詞義又問理由等混合問題。\n\
        分類時，結合必要的前文，判斷學生必須完成什麼操作才能回答。\n\
        前文用來辨認追問的對象，不用來直接決定類型。\n\
        不得依目前階段、學生缺少什麼，或單一疑問詞分類。\n\
        \n\
        六種類型及邊界：\n\
        - clarify：要求學生說清楚原本話語的意思、指涉或適用範圍。核心是「你原本指的是什麼」，不是「你的說法是否成立」。詢問學生原本用詞的標準或界線仍可屬於 clarify。若要求建立或評估衝突價值之間的取捨標準，則屬於 perspective；不能只因學生尚未說明標準，就標記 clarify。\n\
        - reason：要求學生提供支持其立場的理由。核心是「你為什麼支持這個立場」。若追問是在檢查學生已使用的前提、概括或推論是否成立，則屬於 assumption，不因要求說明理由而改成 reason。\n\
        - assumption：檢查學生論證中已使用的前提、概括或推論是否可靠。前提可以明說，也可以隱含，不限於未說出口的假設。例如檢查「A 是否足以推出 B」「是否只有 A 才會造成 B」。若提出具體例外情境，或要求學生找出能挑戰原則的情境，則考慮 counterexample。\n\
        - counterexample：提出可辨識的假設個案或例外情境，要求學生判斷原有原則在該情境下是否仍成立；或要求學生自己找出能挑戰原則的反例或例外情境。模型提出情境時須說明情境條件，不需要是真實事件；要求學生找反例時，不必先替學生給出情境。僅抽象地問「一定成立嗎」且未要求找出情境，屬於 assumption。情境中存在價值衝突，不代表一定是 perspective。\n\
        - perspective：要求學生比較不同立場的考量，或權衡相互衝突的價值、利益，包括決定優先順序、兼顧方式及取捨界線。核心是「這些不同考量應如何比較或取捨」。若前文已明確指出衝突雙方，而追問要求建立或評估該衝突中的合理限制、優先順序或權重標準，即使沒有再次列出雙方，也屬於 perspective。前文有衝突不會讓所有追問都變成 perspective，仍須檢查實際要求的思考操作。只確認學生指的是哪個立場，仍屬於 clarify；只比較兩個詞的意思或兩項工作的難度，不因此屬於 perspective。\n\
        - wrap_up：要求學生整理或回顧已討論的立場、理由、修正或尚存問題。核心是回顧整合，不是引入新的探究。\n\
        \n\
        分類衝突：\n\
        先辨認主要思考操作，不因附帶的「為什麼」「請說明」改變分類。\n\
        只有主要操作確實同時符合多類時，才使用以下優先順序：\n\
        wrap_up > perspective > counterexample > assumption > reason > clarify。\n\
        這個順序不限制追問策略的選擇。\n\
        \n\
        邊界範例：\n\
        學生：「我支持依貢獻公平分配。」\n\
        追問：「你所說的『貢獻』包含哪些事情？」\n\
        → clarify：說清楚原本用詞。\n\
        追問：「你為什麼支持依貢獻分配？」\n\
        → reason：提供支持立場的理由。\n\
        \n\
        學生：「工作時數越長，貢獻就一定越大。」\n\
        追問：「工作時數足以判斷貢獻大小嗎？」\n\
        → assumption：檢查時數與貢獻之間的推論。\n\
        追問：「若甲花兩小時完成的工作比乙八小時還多，你仍認為乙貢獻較大嗎？」\n\
        → counterexample：用具體情境測試原則。\n\
        \n\
        學生：「分配應看貢獻，但也要照顧有急迫需要的人。」\n\
        追問：「你所說的『急迫需要』是指哪些情況？」\n\
        → clarify：釐清原意。\n\
        追問：「當貢獻和急迫需要指向不同人時，你會依什麼標準決定優先順序？」\n\
        → perspective：建立衝突時的取捨標準。\n\
        \n\
        學生：「我支持資訊公開，但也認為個人隱私需要保護；兩者衝突時，我不知道公開範圍應該畫在哪裡。」\n\
        追問：「你所說的『個人隱私』包含哪些資訊？」\n\
        → clarify：說清楚學生原本用詞的範圍。\n\
        追問：「你會用什麼標準判斷公開範圍是否合理？」\n\
        → perspective：承接前文的公開與隱私衝突，要求建立取捨界線；不是只解釋詞義。\n\
        \n\
        學生：「任何時候都不能違反規則。」\n\
        追問：「你能想到一個遵守規則反而不合理的情境嗎？」\n\
        → counterexample：要求學生找反例，模型不必先提出個案。\n\
        追問：「這個原則一定適用於所有情況嗎？」\n\
        → assumption：抽象檢查概括是否成立，未要求找出情境。\n\
        \n\
        上述 assumption 或 perspective 問題即使附加「為什麼」，分類也不會變成 reason。\n\
        \n\
        目前階段（階段只進不退）：{stage}{wrap}\n\n\
        輸出格式（每次回覆都必須完整遵守）：\n\
        第一部分：給學生看的簡短追問。\n\
        第二部分：另起一行，輸出 {delim}。\n\
        第三部分：另起一行，輸出且只輸出一個合法 JSON 物件。\n\
        JSON 必須包含 question_type、advance、reason 三個欄位。\n\
        question_type 必須是六種合法類型之一；advance 必須是 true 或 false；reason 必須是簡短的非空字串。\n\
        不得省略第二或第三部分。不得使用 Markdown 程式碼區塊包住 JSON。\n\
        先前 assistant 訊息可能只保留給學生看的正文；不論歷史訊息是否含有 META，本次都必須輸出完整三部分。\n\
        reason 欄位只說明 advance 的判斷依據，不作為 question_type 的分類依據。\n\
        目前階段的通過條件必須全部已由學生完成，advance 才能為 true。\n\
        若 reason 指出某項必要條件尚未完成，advance 必須為 false。\n\
        advance 僅根據學生截至目前已完成的思考判斷，不根據本次準備提出的問題判斷。\n\
        \n\
        完整格式示例（只示範輸出結構，內容與判斷必須依當前對話重新產生）：\n\
        假設目前是第 1 階段，學生只說「我支持公平分配」，尚未解釋公平，完整回覆如下：\n\
        你所說的「公平」具體是什麼意思？\n\
        {delim}\n\
        {{\"question_type\":\"clarify\",\"advance\":false,\"reason\":\"學生尚未說明公平的意思。\"}}\n\
        \n\
        現在請回覆實際對話。只輸出一份完整回覆：學生看的追問、獨立一行的 {delim}、獨立一行的 JSON。\n\
        追問句結束不代表本次輸出結束；必須接著輸出分隔標記和 JSON，JSON 結束後才完成。\n",
        title = c.title,
        desc = c.description,
        stage = stage,
        wrap = wrap,
        delim = META_DELIM,
    )
}

// ---- OpenAI 實作 ----

#[derive(Clone)]
pub struct OpenAiProvider {
    http: reqwest::Client,
    base_url: String,
    /// 第一個是主模型，其後是備援模型（依序）；每個模型各有自己的 key 池冷卻狀態
    targets: Vec<ModelTarget>,
}

#[derive(Clone)]
struct ModelTarget {
    model: String,
    keys: Arc<KeyPool>,
}

/// 開始串流前遇到限流或伺服器錯誤時，最多換幾把 key 重試。
const MAX_KEY_RETRIES: usize = 3;
/// 所有 key 都在冷卻時，最多願意等多久讓最快恢復的那把回來；更久就直接失敗，
/// 不要讓學生的請求卡在一個遠遠等不到的冷卻上（呼叫端另有首 token 逾時）。
const MAX_KEY_WAIT: Duration = Duration::from_secs(30);

/// 對單一模型送出請求失敗的原因，用來決定要不要換備援模型。
enum SendFailure {
    /// 這個模型被限流：每次失敗都是 429，或它的 key 在先前的 429 之後還在冷卻中。可以換備援模型。
    RateLimited,
    /// 其他原因（連線失敗、5xx、key 被拒絕、請求本身有誤）。換模型無濟於事，不換。
    Failed,
}

impl SendFailure {
    /// 到目前為止的失敗全是 429 就算限流。
    fn from_only_rate_limited(only_rate_limited: bool) -> Self {
        if only_rate_limited {
            Self::RateLimited
        } else {
            Self::Failed
        }
    }
}

impl OpenAiProvider {
    pub fn new(base_url: &str, keys: Arc<KeyPool>, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            targets: vec![ModelTarget {
                model: model.to_string(),
                keys,
            }],
        }
    }

    /// 主模型被限流（429）時依序改用的備援模型。與主模型相同或重複的會略過。
    /// 每個備援模型有獨立的 key 冷卻狀態：主模型的 429 是整個模型的問題，不能讓 key 因此冷卻。
    pub fn with_fallbacks(mut self, models: Vec<String>) -> Self {
        for model in models {
            if self.targets.iter().any(|t| t.model == model) {
                continue;
            }
            let keys = Arc::new(self.targets[0].keys.fork());
            self.targets.push(ModelTarget { model, keys });
        }
        self
    }

    /// 取一把可用的 key；全部冷卻時等最快恢復的那把（不超過 `MAX_KEY_WAIT`）。
    /// `may_wait` 為 false（後面還有備援模型可換）時不等，直接放棄。
    async fn acquire_key(
        keys: &KeyPool,
        may_wait: bool,
    ) -> Result<crate::key_pool::Lease, AiError> {
        for _ in 0..=MAX_KEY_RETRIES {
            if let Some(lease) = keys.acquire() {
                return Ok(lease);
            }
            match keys.next_available_in() {
                Some(wait) if may_wait && wait <= MAX_KEY_WAIT => tokio::time::sleep(wait).await,
                _ => break,
            }
        }
        Err(AiError)
    }

    /// 對一個模型送出請求並回傳成功的回應；429 與 401/402/403 讓該 key 冷卻後換下一把重試，5xx 也換下一把
    /// （但每把 key 在一次請求中最多試一次，避免對故障的供應商連續猛打）。
    /// 只在尚未開始串流時重試，串流開始後的失敗不會重送（已有部分文字給了學生）。
    async fn send_with_failover(
        &self,
        keys: &KeyPool,
        body: &(impl Serialize + Sync),
        may_wait: bool,
    ) -> Result<reqwest::Response, SendFailure> {
        let mut server_errored: Vec<usize> = Vec::new();
        // 到目前為止的失敗是否全是 429。還沒送出請求就沒有 key 可用時，要看冷卻的原因：
        // 是先前的 429 才算限流；key 被拒絕（401/402/403）換模型也沒用
        let mut only_rate_limited = keys.usable_count() > 0 || keys.has_rate_limited_key();
        for _ in 0..=MAX_KEY_RETRIES {
            let Ok(lease) = Self::acquire_key(keys, may_wait).await else {
                // 後面還有備援模型時，這只是要換模型的訊號，不是錯誤
                if may_wait {
                    tracing::error!("no usable AI key");
                } else {
                    tracing::warn!("no usable AI key for this model");
                }
                return Err(SendFailure::from_only_rate_limited(only_rate_limited));
            };
            if server_errored.contains(&lease.index) {
                return Err(SendFailure::Failed);
            }
            tracing::debug!(key = lease.index + 1, "sending AI request");
            let res = self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&lease.key)
                .json(body)
                .send()
                .await
                .map_err(|_| SendFailure::Failed)?;
            let status = res.status();
            if status.is_success() {
                return Ok(res);
            }
            tracing::warn!(status = %status, key = lease.index + 1, "AI provider returned an error status");
            only_rate_limited &= status.as_u16() == 429;
            if status.is_server_error() {
                server_errored.push(lease.index);
            }
            if !keys.report_failure(
                lease.index,
                status.as_u16(),
                parse_retry_after(res.headers()),
            ) {
                return Err(SendFailure::Failed);
            }
        }
        Err(SendFailure::from_only_rate_limited(only_rate_limited))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SseItem {
    Delta(String),
    /// 推理模型的推理內容；文字不保留，只用來表示模型已開始回應
    Reasoning,
    Done,
    Ignore,
    /// 串流內的錯誤區塊（HTTP 已回 200，內容卻是 `error`）。`capacity` 表示容量或限流類的錯誤
    /// （429、gRPC 的 ResourceExhausted），換一個模型有機會成功；其他錯誤換模型沒有幫助。
    /// 供應商的錯誤文字不保留，也不寫入日誌。
    Error {
        capacity: bool,
    },
}

/// 錯誤區塊是不是容量或限流類：code 為 429，或訊息含 ResourceExhausted（大小寫、底線、空白不拘）。
fn is_capacity_error(err: &serde_json::Map<String, serde_json::Value>) -> bool {
    if err.get("code").and_then(|c| c.as_u64()) == Some(429) {
        return true;
    }
    err.get("message")
        .and_then(|m| m.as_str())
        .is_some_and(|m| {
            m.chars()
                .filter(|c| !matches!(c, '_' | ' ' | '-'))
                .collect::<String>()
                .to_ascii_lowercase()
                .contains("resourceexhausted")
        })
}

/// 解析 OpenAI 串流的一行（`data: {...}`）。格式錯誤回傳 `Err`，不會當作正常結束。
pub fn parse_sse_line(line: &str) -> Result<SseItem, AiError> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(SseItem::Ignore); // 註解、空行、其他欄位
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(SseItem::Done);
    }
    let v: serde_json::Value = serde_json::from_str(data).map_err(|_| AiError)?;
    if let Some(text) = v["choices"][0]["delta"]["content"].as_str()
        && !text.is_empty()
    {
        return Ok(SseItem::Delta(text.to_string()));
    }
    if let Some(err) = v["error"].as_object() {
        return Ok(SseItem::Error {
            capacity: is_capacity_error(err),
        });
    }
    // OpenRouter 用 `reasoning`，DeepSeek／vLLM 用 `reasoning_content`
    let delta = &v["choices"][0]["delta"];
    if ["reasoning", "reasoning_content"]
        .iter()
        .any(|k| delta[*k].as_str().is_some_and(|t| !t.is_empty()))
    {
        return Ok(SseItem::Reasoning);
    }
    if v["choices"][0]["finish_reason"].is_string() {
        return Ok(SseItem::Done);
    }
    Ok(SseItem::Ignore)
}

/// 把任意切分的位元組累積成完整的行再解碼，避免多位元組字元被切在片段之間。
#[derive(Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseItem>, AiError> {
        self.buf.extend_from_slice(bytes);
        let mut items = Vec::new();
        // 逐行就地解碼，最後一次丟掉已處理的部分，不為每一行配置新的緩衝
        let mut start = 0;
        while let Some(i) = self.buf[start..].iter().position(|&b| b == b'\n') {
            let line = std::str::from_utf8(&self.buf[start..start + i]).map_err(|_| AiError)?;
            items.push(parse_sse_line(line.trim_end())?);
            start += i + 1;
        }
        self.buf.drain(..start);
        Ok(items)
    }
}

/// 送給供應商的請求本文。直接借用請求的文字序列化，不先複製成 `serde_json::Value`；
/// 欄位依字母排列，與先前 `json!`（BTreeMap）輸出的位元組相同。
#[derive(Serialize)]
struct ChatBody<'a> {
    messages: &'a [ChatMsg<'a>],
    model: &'a str,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMsg<'a> {
    content: &'a str,
    role: &'static str,
}

impl OpenAiProvider {
    /// 對第 `index` 個模型送出請求（含該模型自己的 key 重試）。
    async fn send_to(
        &self,
        index: usize,
        req: &AiRequest,
    ) -> Result<reqwest::Response, SendFailure> {
        let target = &self.targets[index];
        let system = ChatMsg {
            content: &req.system,
            role: "system",
        };
        let turns = req.messages.iter().map(|m| ChatMsg {
            content: &m.content,
            role: if m.role == ChatRole::User {
                "user"
            } else {
                "assistant"
            },
        });
        let messages: Vec<ChatMsg> = std::iter::once(system).chain(turns).collect();
        let body = ChatBody {
            messages: &messages,
            model: &target.model,
            stream: true,
        };
        // 後面還有備援模型時不等 key 冷卻，直接換
        self.send_with_failover(&target.keys, &body, index + 1 == self.targets.len())
            .await
    }

    /// 從第 `start` 個模型起，找第一個能送出請求的；只在限流（429）時往下一個找，
    /// 其他失敗直接放棄。換過模型的會留下日誌。
    async fn first_available(
        &self,
        start: usize,
        req: &AiRequest,
    ) -> Option<(usize, reqwest::Response)> {
        let last = self.targets.len() - 1;
        for i in start..=last {
            match self.send_to(i, req).await {
                Ok(res) => {
                    if i > 0 {
                        tracing::warn!(model = %self.targets[i].model, "AI request served by a fallback model");
                    }
                    return Some((i, res));
                }
                Err(SendFailure::RateLimited) if i < last => {
                    tracing::warn!(
                        model = %self.targets[i].model,
                        next = %self.targets[i + 1].model,
                        "AI model rate limited, trying the next model"
                    );
                }
                Err(_) => return None,
            }
        }
        None
    }

    /// 把成功的串流回應轉成逐段文字的串流。回應是第 `index` 個模型的；
    /// 還沒送出任何內容就收到容量錯誤時，在串流內部改用下一個備援模型。
    fn into_stream(self, res: reqwest::Response, mut index: usize, req: AiRequest) -> AiStream {
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut current = res;
            loop {
                match pump(current, &tx).await {
                    Pump::Finished => return,
                    Pump::Failed => {
                        let _ = tx.send(Err(AiError)).await;
                        return;
                    }
                    Pump::CapacityBeforeContent => {
                        // 呼叫端已經放棄（例如首字逾時後重試）：不要再替沒人讀的回覆消耗備援額度
                        if tx.is_closed() {
                            return;
                        }
                        tracing::warn!(
                            model = %self.targets[index].model,
                            "AI model reported a capacity error before any content, trying the next model"
                        );
                        match self.first_available(index + 1, &req).await {
                            Some((next, res)) => {
                                current = res;
                                index = next;
                            }
                            None => {
                                let _ = tx.send(Err(AiError)).await;
                                return;
                            }
                        }
                    }
                }
            }
        });
        ReceiverStream::new(rx).boxed()
    }
}

/// 一個串流回應讀完的結果。
enum Pump {
    /// 看到結束標記，正常完成
    Finished,
    /// 失敗（連線中斷、格式錯誤、串流內的其他錯誤、已開始輸出後的任何錯誤）；呼叫端要送出錯誤
    Failed,
    /// 還沒送出任何內容（正文或推理）就收到容量錯誤；可以換備援模型
    CapacityBeforeContent,
}

async fn pump(res: reqwest::Response, tx: &mpsc::Sender<Result<String, AiError>>) -> Pump {
    let mut bytes = res.bytes_stream();
    let mut decoder = SseDecoder::default();
    let mut started = false;
    while let Some(chunk) = bytes.next().await {
        let items = match chunk.map_err(|_| AiError).and_then(|c| decoder.push(&c)) {
            Ok(items) => items,
            Err(_) => return Pump::Failed,
        };
        for item in items {
            match item {
                SseItem::Delta(t) => {
                    started = true;
                    if tx.send(Ok(t)).await.is_err() {
                        return Pump::Finished;
                    }
                }
                // 推理模型可能推理數十秒才輸出正文：推理一開始就送一個空片段，
                // 讓呼叫端知道模型已在回應，不套用首字逾時
                SseItem::Reasoning if !started => {
                    started = true;
                    if tx.send(Ok(String::new())).await.is_err() {
                        return Pump::Finished;
                    }
                }
                SseItem::Error { capacity: true } if !started => {
                    return Pump::CapacityBeforeContent;
                }
                SseItem::Error { .. } => return Pump::Failed,
                // 看到結束標記才算正常完成
                SseItem::Done => return Pump::Finished,
                SseItem::Reasoning | SseItem::Ignore => {}
            }
        }
    }
    // 連線在看到結束標記前就斷了：視為失敗，不能當作完整回覆
    Pump::Failed
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        let (index, res) = self.first_available(0, &req).await.ok_or(AiError)?;
        Ok(self.clone().into_stream(res, index, req))
    }
}

// ---- 非串流呼叫與總結 ----

/// 把串流收成完整文字（總結等不需要逐字顯示的用途）；套用與對話相同的逾時規則（S-03.5）。
pub async fn collect(
    ai: &dyn AiProvider,
    req: AiRequest,
    first_token: std::time::Duration,
    total: std::time::Duration,
) -> Result<String, AiError> {
    use tokio::time::{Instant, timeout_at};
    let start = Instant::now();
    let first_deadline = start + first_token;
    let total_deadline = start + total;
    let mut stream = timeout_at(first_deadline, ai.stream_chat(req))
        .await
        .map_err(|_| AiError)??;
    let mut text = String::new();
    let mut got_chunk = false;
    loop {
        let limit = if got_chunk {
            total_deadline
        } else {
            first_deadline.min(total_deadline)
        };
        match timeout_at(limit, stream.next()).await {
            Err(_) | Ok(Some(Err(_))) => return Err(AiError),
            Ok(None) => return Ok(text),
            Ok(Some(Ok(chunk))) => {
                got_chunk = true;
                text.push_str(&chunk);
            }
        }
    }
}

/// 背景用途（總結、主張分組）的 `collect`：只套用完整回覆上限，不套用首個 token 逾時。
/// 推理模型常先推理數十秒才輸出正文，背景工作沒有人在畫面前等第一個字，
/// 套用首字逾時只會讓它無故失敗。
pub async fn collect_background(
    ai: &dyn AiProvider,
    req: AiRequest,
    total: std::time::Duration,
) -> Result<String, AiError> {
    collect(ai, req, total, total).await
}

/// 單一總結欄位的長度上限（字元）。
const MAX_SUMMARY_FIELD_CHARS: usize = 2000;
/// 一句主張的長度上限（字元）。
const MAX_CLAIM_CHARS: usize = 60;
/// 主張群組名稱的長度上限（字元）。
const MAX_GROUP_NAME_CHARS: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub stance: String,
    pub reasons: String,
    /// 學生最終主張的一句短句（分組用）；沒給時為 `None`，不影響總結本身。
    pub claim: Option<String>,
}

pub fn build_summary_prompt(title: &str, description: &str) -> String {
    format!(
        "你是哲學討論的紀錄整理者，使用繁體中文。以下是學生與引導者就「{title}」（{description}）\
         的完整對話。請只依對話內容整理學生的論點，不加入你自己的看法，也不評論論點的對錯。\n\
         輸出一個 JSON 物件，stance、reasons 為不可為空的字串：\n\
         {{\"stance\":\"學生的主要立場\",\"reasons\":\"學生提出的核心理由\",\
         \"claim\":\"學生最終主張的一句短句\"}}\n\
         claim：學生最終主張的一句短句（30 字內，不含理由，例如「該拉桿」「不拉桿」「視情況而定」）。\n\
         只輸出 JSON。"
    )
}

/// 把對話整理成給總結模型看的單一文字。
pub fn format_transcript(turns: &[ChatTurn]) -> String {
    turns
        .iter()
        .map(|t| {
            let who = if t.role == ChatRole::User {
                "學生"
            } else {
                "引導者"
            };
            format!("{who}：{}", t.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 解析總結；缺欄位、空白或格式錯誤都回傳 `None`（視為結果不完整，F-08.4）。
pub fn parse_summary(raw: &str) -> Option<Summary> {
    #[derive(Deserialize)]
    struct Raw {
        stance: String,
        reasons: String,
        claim: Option<String>,
    }
    let r: Raw = extract_json(raw)?;
    Some(Summary {
        stance: clip(&r.stance, MAX_SUMMARY_FIELD_CHARS)?,
        reasons: clip(&r.reasons, MAX_SUMMARY_FIELD_CHARS)?,
        claim: r.claim.and_then(|c| clip(&c, MAX_CLAIM_CHARS)),
    })
}

// ---- 主張分組（暫定）----

/// 把一句新主張歸入同題目下既有的主張群組，或開新群。回傳 JSON `{"group":"群組名稱"}`。
pub fn build_group_assign_prompt(topic: &str, existing: &[String]) -> String {
    let list = if existing.is_empty() {
        "（目前還沒有任何群組）".to_string()
    } else {
        existing
            .iter()
            .map(|n| format!("- {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "你在整理學生對討論題目「{topic}」的主張。使用者訊息是一位學生最終主張的一句短句。\n\
         請判斷它屬於下列哪個既有群組；意思相同或相近就優先歸入既有群組，名稱要原樣照抄。\n\
         只有真的不屬於任何既有群組時，才開新群，並取一個簡短的名稱（12 字內，描述主張本身）。\n\
         既有群組：\n{list}\n\
         只輸出 JSON：{{\"group\":\"群組名稱\"}}"
    )
}

/// 解析歸類結果；名稱與既有群組相同（忽略大小寫與空白）時回傳既有的寫法。
pub fn parse_group_name(raw: &str, existing: &[String]) -> Option<String> {
    #[derive(Deserialize)]
    struct Raw {
        group: String,
    }
    let r: Raw = extract_json(raw)?;
    let name = clip(&r.group, MAX_GROUP_NAME_CHARS)?;
    let same = |a: &str, b: &str| a.trim().to_lowercase() == b.trim().to_lowercase();
    Some(
        existing
            .iter()
            .find(|e| same(e, &name))
            .cloned()
            .unwrap_or(name),
    )
}

/// 重新分組：把某題目下全部主張一次分成幾群並命名。回傳 JSON
/// `{"groups":[{"name":"群組名稱","members":[1,2]}]}`，members 是主張的編號。
pub fn build_regroup_prompt(topic: &str, claims: &[String]) -> String {
    let list = claims
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {c}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "你在整理學生對討論題目「{topic}」的主張。以下是每位學生最終主張的一句短句（已編號）。\n\
         請把意思相同或相近的歸成一群，每群取一個簡短的名稱（12 字內，描述主張本身）；\n\
         群組數量要少而有意義，每個編號只能屬於一個群組。\n\
         主張：\n{list}\n\
         只輸出 JSON：{{\"groups\":[{{\"name\":\"群組名稱\",\"members\":[1,2]}}]}}"
    )
}

/// 解析重新分組結果：回傳 `(群組名稱, 成員的 0 起算索引)`。名稱重複的群組合併，
/// 超出範圍或重複出現的編號忽略，沒有成員的群組丟棄；整體格式不符回傳 `None`。
pub fn parse_regroup(raw: &str, claim_count: usize) -> Option<Vec<(String, Vec<usize>)>> {
    #[derive(Deserialize)]
    struct Group {
        name: String,
        members: Vec<i64>,
    }
    #[derive(Deserialize)]
    struct Raw {
        groups: Vec<Group>,
    }
    let r: Raw = extract_json(raw)?;
    let mut seen = vec![false; claim_count];
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for g in r.groups {
        let Some(name) = clip(&g.name, MAX_GROUP_NAME_CHARS) else {
            continue;
        };
        let members: Vec<usize> = g
            .members
            .into_iter()
            .filter_map(|n| usize::try_from(n - 1).ok())
            .filter(|&i| i < claim_count && !std::mem::replace(&mut seen[i], true))
            .collect();
        if members.is_empty() {
            continue;
        }
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, m)) => m.extend(members),
            None => out.push((name, members)),
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_summary_requires_all_fields() {
        let ok = parse_summary("```json\n{\"stance\":\"拉桿\",\"reasons\":\"功利\"}\n```").unwrap();
        assert_eq!(ok.stance, "拉桿");
        assert_eq!(ok.reasons, "功利");
        // 沒給主張不算結果不完整；舊格式多給的欄位（學派、分數）直接忽略
        assert_eq!(ok.claim, None);
        let extra = parse_summary(
            "{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"該拉桿\",\"framework\":\"deontology\",\"scores\":{\"care\":1}}",
        )
        .unwrap();
        assert_eq!(extra.claim.as_deref(), Some("該拉桿"));
        for bad in [
            "",
            "no json",
            "{\"stance\":\"a\"}",
            "{\"stance\":\"a\",\"reasons\":\"  \"}",
            "{\"stance\":\"a\",\"reasons\":1}",
            "} reversed {",
        ] {
            assert!(parse_summary(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn claim_is_optional_and_trimmed() {
        let with =
            parse_summary("{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"  該拉桿  \"}").unwrap();
        assert_eq!(with.claim.as_deref(), Some("該拉桿"));
        let long = format!(
            "{{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"{}\"}}",
            "長".repeat(200)
        );
        assert_eq!(
            parse_summary(&long).unwrap().claim.unwrap().chars().count(),
            MAX_CLAIM_CHARS
        );
        let blank = parse_summary("{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"  \"}").unwrap();
        assert_eq!(blank.claim, None);
        assert_eq!(
            parse_summary("{\"stance\":\"a\",\"reasons\":\"b\"}")
                .unwrap()
                .claim,
            None
        );
    }

    #[test]
    fn group_name_prefers_the_existing_spelling() {
        let existing = vec!["拉桿".to_string(), "Not pulling".to_string()];
        assert_eq!(
            parse_group_name("{\"group\":\" 拉桿 \"}", &existing).as_deref(),
            Some("拉桿")
        );
        assert_eq!(
            parse_group_name("```{\"group\":\"not PULLING\"}```", &existing).as_deref(),
            Some("Not pulling")
        );
        assert_eq!(
            parse_group_name("{\"group\":\"視情況而定\"}", &existing).as_deref(),
            Some("視情況而定")
        );
        for bad in ["", "no json", "{\"group\":\"  \"}", "{\"group\":1}"] {
            assert!(parse_group_name(bad, &existing).is_none(), "{bad}");
        }
    }

    #[test]
    fn regroup_ignores_bad_indexes_and_merges_same_names() {
        let raw = "{\"groups\":[{\"name\":\"拉桿\",\"members\":[1,3,9,0]},{\"name\":\"不拉\",\"members\":[2,3]},{\"name\":\"拉桿\",\"members\":[4]},{\"name\":\"空\",\"members\":[]}]}";
        let groups = parse_regroup(raw, 4).unwrap();
        assert_eq!(
            groups,
            vec![
                ("拉桿".to_string(), vec![0, 2, 3]),
                ("不拉".to_string(), vec![1])
            ]
        );
        assert!(parse_regroup("{\"groups\":[]}", 4).is_none());
        assert!(parse_regroup("not json", 4).is_none());
    }

    #[test]
    fn transcript_labels_speakers() {
        let t = format_transcript(&[
            ChatTurn {
                role: ChatRole::User,
                content: "我拉桿".into(),
            },
            ChatTurn {
                role: ChatRole::Assistant,
                content: "為什麼？".into(),
            },
        ]);
        assert_eq!(t, "學生：我拉桿\n\n引導者：為什麼？");
    }

    fn run(chunks: &[&str]) -> (String, String) {
        let mut s = MetaSplitter::default();
        let mut emitted = String::new();
        for c in chunks {
            emitted.push_str(&s.push(c));
        }
        let (rest, meta) = s.finish();
        emitted.push_str(&rest);
        (emitted, meta)
    }

    #[test]
    fn splitter_streams_text_and_keeps_meta_private() {
        let (text, meta) = run(&[
            "你覺得",
            "什麼是正義？\n<<<ME",
            "TA>>>\n{\"advance\":",
            "false}",
        ]);
        assert_eq!(text, "你覺得什麼是正義？\n");
        assert_eq!(meta.trim(), "{\"advance\":false}");
    }

    #[test]
    fn splitter_does_not_swallow_partial_delimiter_lookalikes() {
        let (text, meta) = run(&["a <<< b", " c"]);
        assert_eq!(text, "a <<< b c");
        assert!(meta.is_empty());
    }

    #[test]
    fn splitter_handles_multibyte_boundaries() {
        let (text, _) = run(&["正義是什麼", "？<", "<<META>>>{}"]);
        assert_eq!(text, "正義是什麼？");
    }

    #[test]
    fn parse_meta_accepts_valid_and_falls_back_on_garbage() {
        let m = parse_meta(
            "\n{\"question_type\":\"reason\",\"advance\":true,\"reason\":\" 已說明理由 \"}\n",
        );
        assert_eq!(m.question_type.as_deref(), Some("reason"));
        assert!(m.advance);
        assert_eq!(m.reason.as_deref(), Some("已說明理由"));
        let m = parse_meta("{\"question_type\":\"weird\",\"advance\":true}");
        assert_eq!(m.question_type, None);
        assert_eq!(m.reason, None);
        let m = parse_meta("} reversed {");
        assert!(!m.advance);
        let m = parse_meta("not json");
        assert_eq!(
            m,
            TurnMeta {
                question_type: None,
                advance: false,
                reason: None
            }
        );
    }

    #[test]
    fn parse_meta_truncates_long_reason() {
        let long = "理".repeat(MAX_REASON_CHARS + 50);
        let m = parse_meta(&format!("{{\"advance\":false,\"reason\":\"{long}\"}}"));
        assert_eq!(m.reason.unwrap().chars().count(), MAX_REASON_CHARS);
    }

    #[test]
    fn sse_line_parsing() {
        assert_eq!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{\"content\":\"嗨\"}}]}"),
            Ok(SseItem::Delta("嗨".to_string()))
        );
        assert_eq!(parse_sse_line("data: [DONE]"), Ok(SseItem::Done));
        assert_eq!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}"),
            Ok(SseItem::Done)
        );
        assert_eq!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{}}]}"),
            Ok(SseItem::Ignore)
        );
        assert_eq!(parse_sse_line(": keep-alive"), Ok(SseItem::Ignore));
        // 壞掉的 JSON 不能被當作正常內容或正常結束
        assert_eq!(parse_sse_line("data: {oops"), Err(AiError));
    }

    #[test]
    fn reasoning_deltas_parse_as_reasoning_without_their_text() {
        // OpenRouter 用 `reasoning`（正文是空字串），DeepSeek／vLLM 用 `reasoning_content`
        assert_eq!(
            parse_sse_line(
                "data: {\"choices\":[{\"delta\":{\"content\":\"\",\"reasoning\":\"想一下\"}}]}"
            ),
            Ok(SseItem::Reasoning)
        );
        assert_eq!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想一下\"}}]}"),
            Ok(SseItem::Reasoning)
        );
        assert_eq!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{\"reasoning\":\"\"}}]}"),
            Ok(SseItem::Ignore)
        );
    }

    #[test]
    fn error_chunks_are_classified_as_capacity_errors_or_other_errors() {
        let err = |code: u32, msg: &str| {
            format!("data: {{\"choices\":[],\"error\":{{\"code\":{code},\"message\":\"{msg}\"}}}}")
        };
        let capacity = Ok(SseItem::Error { capacity: true });
        let other = Ok(SseItem::Error { capacity: false });
        // 實測：NVIDIA 的並發上限用 HTTP 200 加 502 ResourceExhausted 的區塊送出
        assert_eq!(
            parse_sse_line(&err(
                502,
                "Upstream error from Nvidia: ResourceExhausted: Worker local total request limit reached (16/16)"
            )),
            capacity
        );
        // 寫法不同的 gRPC 狀態名稱
        assert_eq!(parse_sse_line(&err(503, "RESOURCE_EXHAUSTED")), capacity);
        assert_eq!(parse_sse_line(&err(500, "resource exhausted")), capacity);
        assert_eq!(parse_sse_line(&err(429, "slow down")), capacity);
        assert_eq!(parse_sse_line(&err(502, "Upstream error: Internal")), other);
        assert_eq!(parse_sse_line(&err(400, "bad request")), other);
        // 有 error 但沒有 code 也是錯誤，只是不算容量錯誤
        assert_eq!(
            parse_sse_line("data: {\"choices\":[],\"error\":{\"message\":\"boom\"}}"),
            other
        );
        // 帶有正文的區塊仍以正文為準，不因為多了 error 欄位而被丟掉
        assert_eq!(
            parse_sse_line("data: {\"choices\":[{\"delta\":{\"content\":\"嗨\"}}],\"error\":null}"),
            Ok(SseItem::Delta("嗨".to_string()))
        );
    }

    #[test]
    fn decoder_handles_multibyte_char_split_across_chunks() {
        let line = "data: {\"choices\":[{\"delta\":{\"content\":\"正義\"}}]}\n";
        let bytes = line.as_bytes();
        // 切在「正」的中間
        let cut = line.find('正').unwrap() + 1;
        let mut d = SseDecoder::default();
        assert_eq!(d.push(&bytes[..cut]), Ok(vec![]));
        assert_eq!(
            d.push(&bytes[cut..]),
            Ok(vec![SseItem::Delta("正義".to_string())])
        );
    }

    #[test]
    fn decoder_rejects_invalid_utf8_and_bad_json() {
        let mut d = SseDecoder::default();
        assert_eq!(d.push(b"data: \xff\xfe\n"), Err(AiError));
        let mut d = SseDecoder::default();
        assert_eq!(d.push(b"data: {oops\n"), Err(AiError));
    }

    #[test]
    fn prompt_reflects_stage_and_wrap_up() {
        let c = PromptContext {
            title: "T",
            description: "D",
            stage: 2,
            turn: 3,
            wrap_up_turn: 15,
        };

        let p = build_system_prompt(&c);

        // 1. 確認討論題目和階段正確。
        assert!(p.contains("討論題目：T"));
        assert!(p.contains("題目說明：D"));
        assert!(p.contains("第 2 階段「論證」"));
        assert!(!p.contains("開始引導學生整理"));

        // 2. 確認六種追問類型都有定義。
        for question_type in [
            "clarify",
            "reason",
            "assumption",
            "counterexample",
            "perspective",
            "wrap_up",
        ] {
            assert!(
                p.contains(&format!("- {question_type}：")),
                "缺少追問類型定義：{question_type}"
            );
        }

        // 3. 確認目前版本的分類規則存在；此測試不評估模型實際行為。
        // 檢查規則內容，不綁定段落標題。
        for rule in [
            "再根據實際追問標記 question_type",
            "不得依目前階段、學生缺少什麼，或單一疑問詞分類",
            "權衡相互衝突的價值、利益",
            "要求學生判斷原有原則在該情境下是否仍成立",
            "檢查學生論證中已使用的前提、概括或推論是否可靠",
        ] {
            assert!(p.contains(rule), "缺少分類規則：{rule}");
        }

        // 4. 確認 META 輸出格式要求存在。
        assert!(p.contains(META_DELIM));
        assert!(p.contains("JSON 必須包含 question_type、advance、reason"));
        assert!(p.contains("不得省略第二或第三部分"));

        // 5. 確認 advance 根據學生已完成的思考判斷。
        assert!(p.contains("學生截至目前已完成的思考"));

        // 6. 到達指定回合後，應開始引導收尾。
        let p = build_system_prompt(&PromptContext { turn: 15, ..c });

        assert!(p.contains("開始引導學生整理"));
    }

    /// 等 `delay` 之後才給出一段文字的假 AI，模擬推理模型先想很久才開始輸出。
    struct SlowStart(std::time::Duration);

    #[async_trait]
    impl AiProvider for SlowStart {
        async fn stream_chat(&self, _: AiRequest) -> Result<AiStream, AiError> {
            let delay = self.0;
            Ok(futures_util::stream::once(async move {
                tokio::time::sleep(delay).await;
                Ok("{}".to_string())
            })
            .boxed())
        }
    }

    fn any_request() -> AiRequest {
        AiRequest {
            system: String::new(),
            messages: vec![],
        }
    }

    #[tokio::test]
    async fn collect_waits_for_a_slow_first_chunk_within_the_total_limit() {
        let ai = SlowStart(std::time::Duration::from_millis(300));
        let text = collect_background(&ai, any_request(), std::time::Duration::from_secs(2)).await;
        assert_eq!(text.unwrap(), "{}");
    }

    #[tokio::test]
    async fn collect_fails_past_the_total_limit() {
        let ai = SlowStart(std::time::Duration::from_secs(2));
        let text =
            collect_background(&ai, any_request(), std::time::Duration::from_millis(200)).await;
        assert!(text.is_err());
    }
}
