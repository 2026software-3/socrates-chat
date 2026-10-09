//! AI 服務介面（S-03.1、S-08.1）：所有 AI 呼叫都經過 `AiProvider`，測試時注入假實作。
//!
//! 本檔另含蘇格拉底式追問的提示（S-03.2）與每回合結構化結果的解析。

use async_trait::async_trait;
use futures_util::{StreamExt, stream::BoxStream};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// 提示與規則的版本；記錄在每則 AI 回覆上（S-03.2），修改提示時一併更新。
pub const RULES_VERSION: &str = "v1.10/zh-TW";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
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

pub type AiStream = BoxStream<'static, Result<String, AiError>>;

#[async_trait]
pub trait AiProvider: Send + Sync {
    /// 開始串流回覆；回傳的串流依序給出文字片段。
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
        let cut = self.held.len() - keep;
        let text = self.held[..cut].to_string();
        self.held.drain(..cut);
        text
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
    let parsed = raw
        .find('{')
        .zip(raw.rfind('}'))
        .filter(|(a, b)| a <= b)
        .and_then(|(a, b)| serde_json::from_str::<Raw>(&raw[a..=b]).ok());
    match parsed {
        Some(r) => TurnMeta {
            question_type: r
                .question_type
                .filter(|q| QUESTION_TYPES.contains(&q.as_str())),
            advance: r.advance,
            reason: r
                .reason
                .map(|s| s.trim().chars().take(MAX_REASON_CHARS).collect::<String>())
                .filter(|s| !s.is_empty()),
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
        - 語氣中立、尊重，回覆簡短；離題時引導回主題。\n\n\
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

pub struct OpenAiProvider {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SseItem {
    Delta(String),
    Done,
    Ignore,
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
        while let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            let line = std::str::from_utf8(&line).map_err(|_| AiError)?;
            items.push(parse_sse_line(line.trim_end())?);
        }
        Ok(items)
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        let mut messages = vec![json!({"role": "system", "content": req.system})];
        messages.extend(req.messages.iter().map(|m| {
            json!({
                "role": if m.role == ChatRole::User { "user" } else { "assistant" },
                "content": m.content,
            })
        }));
        let res = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({"model": self.model, "stream": true, "messages": messages}))
            .send()
            .await
            .map_err(|_| AiError)?;
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), "AI provider returned an error status");
            return Err(AiError);
        }
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut bytes = res.bytes_stream();
            let mut decoder = SseDecoder::default();
            while let Some(chunk) = bytes.next().await {
                let items = match chunk.map_err(|_| AiError).and_then(|c| decoder.push(&c)) {
                    Ok(items) => items,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };
                for item in items {
                    match item {
                        SseItem::Delta(t) => {
                            if tx.send(Ok(t)).await.is_err() {
                                return;
                            }
                        }
                        // 看到結束標記才算正常完成
                        SseItem::Done => return,
                        SseItem::Ignore => {}
                    }
                }
            }
            // 連線在看到結束標記前就斷了：視為失敗，不能當作完整回覆
            let _ = tx.send(Err(AiError)).await;
        });
        Ok(ReceiverStream::new(rx).boxed())
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
    /// 論點分布用的分類（暫定）：模型沒給或給了不認得的值時為 `None`，不影響總結本身。
    /// 學生最終主張的一句短句（分組用）；沒給時為 `None`。
    pub claim: Option<String>,
    pub framework: Option<String>,
    /// 6 個學派維度的分數（順序同 `SCHOOLS`，各 0–5）；沒給完整分數時為 `None`。
    pub scores: Option<[u8; 6]>,
}

/// 雷達圖的 6 個維度（S-04.2 暫定）：效益主義、義務論、德行倫理、契約論、關懷倫理、存在主義。
pub const SCHOOLS: [&str; 6] = [
    "utilitarianism",
    "deontology",
    "virtue",
    "contractarianism",
    "care",
    "existentialism",
];

/// 每個維度分數的上限。
pub const MAX_SCORE: u8 = 5;

/// 解析 6 維分數：必須是含 6 個學派的物件、值都是數字；超出範圍的值夾到 0–5。任何一項缺漏就整組不用。
fn parse_scores(v: &serde_json::Value) -> Option<[u8; 6]> {
    let obj = v.as_object()?;
    let mut out = [0u8; 6];
    for (slot, key) in out.iter_mut().zip(SCHOOLS) {
        let n = obj.get(key)?.as_f64()?;
        *slot = n.round().clamp(0.0, f64::from(MAX_SCORE)) as u8;
    }
    Some(out)
}

/// 論證主要依循的倫理學派（S-04.2 的 6 個維度），其他或無法歸類為 `other`。
pub const FRAMEWORKS: [&str; 7] = [
    "utilitarianism",
    "deontology",
    "virtue",
    "contractarianism",
    "care",
    "existentialism",
    "other",
];

pub fn build_summary_prompt(title: &str, description: &str) -> String {
    format!(
        "你是哲學討論的紀錄整理者，使用繁體中文。以下是學生與引導者就「{title}」（{description}）\
         的完整對話。請只依對話內容整理學生的論點，不加入你自己的看法，也不評論論點的對錯。\n\
         輸出一個 JSON 物件，stance、reasons 為不可為空的字串：\n\
         {{\"stance\":\"學生的主要立場\",\"reasons\":\"學生提出的核心理由\",\
         \"claim\":\"學生最終主張的一句短句\",\
         \"framework\":\"utilitarianism|deontology|virtue|contractarianism|care|existentialism|other\",\
         \"scores\":{{\"utilitarianism\":0,\"deontology\":0,\"virtue\":0,\"contractarianism\":0,\"care\":0,\"existentialism\":0}}}}\n\
         claim：學生最終主張的一句短句（30 字內，不含理由，例如「該拉桿」「不拉桿」「視情況而定」）。\n\
         framework：學生論證主要依循的倫理學派——效益主義 utilitarianism、義務論 deontology、德行倫理 virtue、\
         契約論 contractarianism、關懷倫理 care、存在主義 existentialism；無法歸類時用 other。\n\
         scores：替你整理出的這份論點總結（stance 與 reasons）在 6 個學派上各打 0 到 5 的整數分——0 表示論點完全沒用到該學派的思路，5 表示是論點的核心；\
         依學生「實際提出」的立場與理由評分，不是你認為正確的答案；6 個學派都要給分。\n\
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
        framework: Option<String>,
        scores: Option<serde_json::Value>,
    }
    let (start, end) = (raw.find('{')?, raw.rfind('}')?);
    if end < start {
        return None;
    }
    let r: Raw = serde_json::from_str(&raw[start..=end]).ok()?;
    let clean = |s: String| -> Option<String> {
        let s: String = s.trim().chars().take(MAX_SUMMARY_FIELD_CHARS).collect();
        (!s.is_empty()).then_some(s)
    };
    Some(Summary {
        stance: clean(r.stance)?,
        reasons: clean(r.reasons)?,
        claim: r
            .claim
            .map(|c| c.trim().chars().take(MAX_CLAIM_CHARS).collect::<String>())
            .filter(|c| !c.is_empty()),
        framework: r.framework.filter(|f| FRAMEWORKS.contains(&f.as_str())),
        scores: r.scores.as_ref().and_then(parse_scores),
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
    let (start, end) = (raw.find('{')?, raw.rfind('}')?);
    if end < start {
        return None;
    }
    let r: Raw = serde_json::from_str(&raw[start..=end]).ok()?;
    let name: String = r.group.trim().chars().take(MAX_GROUP_NAME_CHARS).collect();
    if name.is_empty() {
        return None;
    }
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
    let (start, end) = (raw.find('{')?, raw.rfind('}')?);
    if end < start {
        return None;
    }
    let r: Raw = serde_json::from_str(&raw[start..=end]).ok()?;
    let mut seen = vec![false; claim_count];
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for g in r.groups {
        let name: String = g.name.trim().chars().take(MAX_GROUP_NAME_CHARS).collect();
        if name.is_empty() {
            continue;
        }
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
        // 沒給分類不算結果不完整
        assert_eq!((ok.claim, ok.framework), (None, None));
        let classified =
            parse_summary("{\"stance\":\"a\",\"reasons\":\"b\",\"framework\":\"deontology\"}")
                .unwrap();
        assert_eq!(classified.framework.as_deref(), Some("deontology"));
        assert_eq!(classified.framework.as_deref(), Some("deontology"));
        // 6 維分數：四捨五入並夾在 0–5；缺一個學派就整組不用
        let scored = parse_summary(
            "{\"stance\":\"a\",\"reasons\":\"b\",\"scores\":{\"utilitarianism\":5,\"deontology\":2.6,\"virtue\":-3,\"contractarianism\":9,\"care\":0,\"existentialism\":1}}",
        )
        .unwrap();
        assert_eq!(scored.scores, Some([5, 3, 0, 5, 0, 1]));
        let partial =
            parse_summary("{\"stance\":\"a\",\"reasons\":\"b\",\"scores\":{\"utilitarianism\":5}}")
                .unwrap();
        assert_eq!(partial.scores, None, "分數不完整不影響總結本身");
        let text = parse_summary(
            "{\"stance\":\"a\",\"reasons\":\"b\",\"scores\":{\"utilitarianism\":\"high\",\"deontology\":1,\"virtue\":1,\"contractarianism\":1,\"care\":1,\"existentialism\":1}}",
        )
        .unwrap();
        assert_eq!(text.scores, None);
        // 不認得的分類值直接丟棄
        let odd = parse_summary("{\"stance\":\"a\",\"reasons\":\"b\",\"framework\":7}");
        assert!(
            odd.is_none(),
            "wrong-typed framework makes the JSON invalid"
        );
        let odd =
            parse_summary("{\"stance\":\"a\",\"reasons\":\"b\",\"framework\":\"zen\"}").unwrap();
        assert_eq!((odd.claim, odd.framework), (None, None));
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
}
