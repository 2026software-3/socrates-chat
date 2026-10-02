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
pub const RULES_VERSION: &str = "v1/zh-TW";

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
         - 每次回覆只問「一個」問題，用追問引導學生思考。\n\
         - 絕不提出你自己的答案或立場。學生問「你覺得呢？」時，改為反問，或平衡列出不同觀點但不下結論。\n\
         - 語氣中立、尊重，不使用侮辱或威脅的表達。\n\
         - 學生離題時，簡短回應後把話題帶回題目。\n\
         - 回覆要簡短（幾句話）。\n\n\
         目前階段（階段只進不退）：{stage}{wrap}\n\n\
         回覆格式：先寫給學生看的文字；接著另起一行輸出 {delim}，之後輸出一行 JSON：\n\
         {{\"question_type\":\"clarify|reason|assumption|counterexample|perspective|wrap_up\",\"advance\":true或false,\"reason\":\"一句話說明判斷理由\"}}\n\
         advance 表示學生「到目前為止」是否已達成本階段進入下一階段的條件。JSON 不要給學生看。",
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(p.contains("論證") && !p.contains("開始引導學生整理"));
        let p = build_system_prompt(&PromptContext { turn: 15, ..c });
        assert!(p.contains("開始引導學生整理"));
    }
}
