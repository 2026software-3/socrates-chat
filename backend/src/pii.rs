//! S-02.6：送往外部 AI 服務前的個資遮蔽。
//!
//! 規則式（正規表示式），只遮蔽出站內容：資料庫保存的訊息與學生畫面維持原文。
//! 以 `MaskingAi` 包住 `AiProvider`，所有 AI 呼叫（對話追問、總結、主張分組）都經過同一個遮蔽點。
//! 規則有變動時遞增 `ai::RULES_VERSION`。

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use regex::Regex;

use crate::ai::{AiError, AiProvider, AiRequest, AiStream, ChatTurn};

/// 台灣市話區碼（去掉開頭的 0）；較長的放前面，讓 `037` 不會先被當成 `03`。
const AREA: &str = "(?:826|836|37|49|82|89|[2-8])";

/// 市話號碼 6–8 碼，中間可有一個空白或 `-`（例如 `2345-6789`）。
const LOCAL: &str = "(?:[0-9]{2,4}[ -][0-9]{4}|[0-9]{6,8})";

/// 依比對順序排列的規則：身分證字號、學號、電子郵件、電話（S-02.6）。
fn rules() -> [(String, &'static str); 4] {
    let mobile = "(?:\\+?886[ -]?|0)9(?:[ -]?[0-9]){8}";
    let landline = format!("(?:\\+?886[ -]?\\(?{AREA}\\)?|\\(?0{AREA}\\)?)[ -]?{LOCAL}");
    [
        ("[A-Za-z][12][0-9]{8}".to_string(), "[NATIONAL_ID]"),
        ("[0-9]{8}[A-Za-z]".to_string(), "[STUDENT_ID]"),
        (
            "[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\\.[A-Za-z0-9-]+)+".to_string(),
            "[EMAIL]",
        ),
        (format!("{mobile}|{landline}"), "[PHONE]"),
    ]
}

/// 編譯好的遮蔽規則。
pub struct Masker {
    rules: Vec<(Regex, &'static str)>,
}

impl Masker {
    fn new() -> Result<Self, regex::Error> {
        let rules = rules()
            .into_iter()
            .map(|(pattern, placeholder)| Ok((Regex::new(&pattern)?, placeholder)))
            .collect::<Result<_, regex::Error>>()?;
        Ok(Self { rules })
    }

    /// 依序套用每條規則；佔位詞本身不符合任何規則，已遮蔽的片段不會被重複處理。
    pub fn mask(&self, text: &str) -> String {
        self.rules
            .iter()
            .fold(text.to_string(), |text, (re, placeholder)| {
                replace_isolated(&text, re, placeholder)
            })
    }
}

/// 只取代前後都沒有緊鄰英數字的比對結果，避免把較長的數字串或英數混合字串切一段遮掉。
fn replace_isolated(text: &str, re: &Regex, placeholder: &str) -> String {
    let isolated = |c: Option<char>| !c.is_some_and(|c| c.is_ascii_alphanumeric());
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut from) = (0, 0);
    while let Some(m) = re.find_at(text, from) {
        if isolated(text[..m.start()].chars().next_back())
            && isolated(text[m.end()..].chars().next())
        {
            out.push_str(&text[copied..m.start()]);
            out.push_str(placeholder);
            (copied, from) = (m.end(), m.end());
        } else {
            // 這個位置不成立：往後挪一個字元再找
            from = m.start() + text[m.start()..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// 規則只編譯一次；編譯失敗時為 `None`，之後的 AI 呼叫一律失敗而不送原文。
static MASKER: LazyLock<Option<Masker>> = LazyLock::new(|| {
    Masker::new()
        .inspect_err(|_| tracing::error!("pii masking rules failed to compile"))
        .ok()
});

/// 把文字中的個資換成佔位詞。
pub fn mask(text: &str) -> Option<String> {
    MASKER.as_ref().map(|m| m.mask(text))
}

/// 在送出前遮蔽請求中所有文字（系統提示與每則訊息）的 `AiProvider`。
pub struct MaskingAi {
    inner: Arc<dyn AiProvider>,
    masker: Option<&'static Masker>,
}

impl MaskingAi {
    pub fn new(inner: Arc<dyn AiProvider>) -> Self {
        Self {
            inner,
            masker: MASKER.as_ref(),
        }
    }
}

#[async_trait]
impl AiProvider for MaskingAi {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        // 遮蔽不可用時請求失敗，依 S-03.5 的外部服務失敗處理；不得改送原文
        let masker = self.masker.ok_or(AiError)?;
        let req = AiRequest {
            system: masker.mask(&req.system),
            messages: req
                .messages
                .into_iter()
                .map(|t| ChatTurn {
                    role: t.role,
                    content: masker.mask(&t.content),
                })
                .collect(),
        };
        self.inner.stream_chat(req).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::ai::{ChatRole, ChatTurn};

    fn masked(text: &str) -> String {
        mask(text).expect("rules compile")
    }

    #[test]
    fn masks_emails() {
        for email in [
            "student@example.com",
            "first.last+tag@mail.example.edu.tw",
            "A_B-c@sub-domain.example.org",
        ] {
            assert_eq!(
                masked(&format!("我的信箱是 {email}。")),
                "我的信箱是 [EMAIL]。"
            );
        }
    }

    #[test]
    fn masks_taiwan_mobile_numbers() {
        for phone in [
            "0912345678",
            "0912-345-678",
            "0912 345 678",
            "+886912345678",
            "+886 912 345 678",
            "886912345678",
        ] {
            assert_eq!(
                masked(&format!("電話{phone}，")),
                "電話[PHONE]，",
                "{phone}"
            );
        }
    }

    #[test]
    fn masks_taiwan_landline_numbers() {
        for phone in [
            "0223456789",
            "02-23456789",
            "02-2345-6789",
            "(02)2345-6789",
            "(02) 23456789",
            "037-123456",
            "(037)123456",
            "049 2345678",
            "0826-123456",
            "08-7654321",
            "+886-2-2345-6789",
            "+886 2 23456789",
            "+886(2)23456789",
        ] {
            assert_eq!(
                masked(&format!("請打 {phone} 找我")),
                "請打 [PHONE] 找我",
                "{phone}"
            );
        }
    }

    #[test]
    fn masks_national_ids_in_either_case() {
        for id in ["A123456789", "a223456789", "Z198765432"] {
            assert_eq!(
                masked(&format!("身分證{id}")),
                "身分證[NATIONAL_ID]",
                "{id}"
            );
        }
    }

    #[test]
    fn masks_student_ids_in_either_case() {
        for id in ["11234567A", "41347064s"] {
            assert_eq!(masked(&format!("學號：{id}")), "學號：[STUDENT_ID]", "{id}");
        }
    }

    #[test]
    fn masks_several_kinds_in_one_message() {
        assert_eq!(
            masked("我是 41347064S，信箱 s@example.com，手機 0912-345-678，身分證 A123456789。"),
            "我是 [STUDENT_ID]，信箱 [EMAIL]，手機 [PHONE]，身分證 [NATIONAL_ID]。"
        );
    }

    #[test]
    fn does_not_mask_when_touching_other_letters_or_digits() {
        for text in [
            "X0912345678",
            "09123456789",
            "0912345678a",
            "BA123456789",
            "A1234567890",
            "911234567A",
            "11234567AB",
            "021234567890123",
        ] {
            assert_eq!(masked(text), text, "{text}");
        }
    }

    #[test]
    fn leaves_names_years_amounts_dates_and_long_numbers_alone() {
        for text in [
            "康德和彌爾對這題的看法不同，王小明也同意。",
            "2024 年的調查",
            "西元前 399 年蘇格拉底受審",
            "獎學金 NT$120,000 元",
            "要花 30000 元",
            "2024-05-06 上課",
            "2024/05/06",
            "5月6日 14:30",
            "編號 123456789012345",
            "電車上有 5 個人和 1 個人",
            "[EMAIL] 和 [PHONE] 已經遮蔽過",
        ] {
            assert_eq!(masked(text), text, "{text}");
        }
    }

    #[test]
    fn our_own_prompts_are_left_unchanged() {
        use crate::ai::{
            PromptContext, build_group_assign_prompt, build_regroup_prompt, build_summary_prompt,
            build_system_prompt,
        };
        for stage in 1..=3 {
            let prompt = build_system_prompt(&PromptContext {
                title: "電車難題",
                description: "你會拉桿嗎？",
                stage,
                turn: 18,
                wrap_up_turn: 16,
            });
            assert_eq!(masked(&prompt), prompt);
        }
        for prompt in [
            build_summary_prompt("電車難題", "你會拉桿嗎？"),
            build_group_assign_prompt("電車難題", &["拉桿".to_string()]),
            build_regroup_prompt("電車難題", &["該拉桿".to_string(), "不拉".to_string()]),
        ] {
            assert_eq!(masked(&prompt), prompt);
        }
    }

    /// 記錄收到的請求、一律回空串流的假 AI。
    #[derive(Default)]
    struct Recorder(Mutex<Vec<AiRequest>>);

    #[async_trait]
    impl AiProvider for Recorder {
        async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
            self.0.lock().unwrap().push(req);
            Ok(Box::pin(futures_util::stream::empty()))
        }
    }

    fn request() -> AiRequest {
        AiRequest {
            system: "題目說明：聯絡 teacher@example.com".to_string(),
            messages: vec![
                ChatTurn {
                    role: ChatRole::User,
                    content: "我的手機是 0912345678".to_string(),
                },
                ChatTurn {
                    role: ChatRole::Assistant,
                    content: "為什麼？".to_string(),
                },
            ],
        }
    }

    #[tokio::test]
    async fn masking_ai_masks_the_system_prompt_and_every_message() {
        let inner = Arc::new(Recorder::default());
        let ai = MaskingAi::new(inner.clone());
        assert!(ai.stream_chat(request()).await.is_ok());

        let sent = inner.0.lock().unwrap();
        assert_eq!(sent[0].system, "題目說明：聯絡 [EMAIL]");
        assert_eq!(sent[0].messages[0].content, "我的手機是 [PHONE]");
        assert_eq!(sent[0].messages[1].content, "為什麼？");
    }

    #[tokio::test]
    async fn without_usable_rules_nothing_is_sent() {
        let inner = Arc::new(Recorder::default());
        let ai = MaskingAi {
            inner: inner.clone(),
            masker: None,
        };
        assert!(ai.stream_chat(request()).await.is_err());
        assert!(inner.0.lock().unwrap().is_empty(), "原文不得送出");
    }
}
