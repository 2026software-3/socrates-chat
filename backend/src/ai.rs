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
/// v1.12：對話、總結與主張分組依對話語言輸出，版本後綴記錄語言。
/// v1.13：學生訊息可能來自語音辨識逐字稿或打字，一律依原意理解、不糾正錯字（F-10.1）。
/// v1.14：回覆寫成適合朗讀的口語短句；學生話沒說完就被送出時請他說完（F-10.2）。
/// v1.15：整合 F-08.2：第 3 階段收斂條件判定、禁止反例套娃，達引導收尾回合時由 AI 主動梳理總結（三語一致）。
/// v1.16：以蘇格拉底第一人稱的人設與語氣說話（假裝無知、從內部指出矛盾、坦率承認），規則與輸出格式不變。
/// v1.17：每次回覆先用半句話承接學生的話再問那一個問題，範例改為帶人設的回覆；仍維持兩三句內。
/// v1.18：第 1、2 階段的通過條件各只有兩個且不得自創額外條件，條件達成即晉級；承接只回應當回合、句型要變化、問題用開放式。
/// v1.19：第 3 階段要學生針對「具體」反例實質回應才算收斂（承認假設、重述立場、只答是否都不算）；承接限半句約 15 字、句型要變化；收尾回顧限三句且不重複。
/// v1.20：學生對同一個具體情境第二次回應仍不夠具體時，改用不同角度的具體情境，不再第三次回到同一情境；禁止反例套娃僅適用於學生已實質回應之後。
/// v1.21：學生開始往具體方向思考時先順著他提到的那一點追問，不換情境、不說他不夠具體；只有連續兩次含糊且沒有具體方向才換情境。
/// v1.22：壓縮三語第 3 階段與收尾的重複規則（收尾回顧、順著學生、換情境各只寫一次），行為規則不變。
/// v1.23：再精簡三語提示：第 3 階段括號舉例、輸出格式的重複說明、第 1／1b 輸出範例合併、人設殘餘重複；行為規則不變。
pub const RULES_VERSION: &str = "v1.23/zh-TW";

/// 對話語言對應的提示版本；未知值保守地退回繁體中文。
pub fn rules_version(language: &str) -> &'static str {
    match language {
        "en" => "v1.23/en",
        "es" => "v1.23/es",
        _ => RULES_VERSION,
    }
}

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
    pub language: &'a str,
}

pub fn build_system_prompt(c: &PromptContext) -> String {
    match c.language {
        "en" => build_system_prompt_en(c),
        "es" => build_system_prompt_es(c),
        _ => build_system_prompt_zh(c),
    }
}

fn build_system_prompt_zh(c: &PromptContext) -> String {
    let is_wrap_up = c.turn >= c.wrap_up_turn as i32;
    let stage = if is_wrap_up {
        let stage_name = match c.stage {
            1 => "第 1 階段「釐清」",
            2 => "第 2 階段「論證」",
            _ => "第 3 階段「挑戰」",
        };
        format!(
            "{stage_name}（已達引導收尾回合，第 15 回合起）：\n\
             【回合上限最高優先級覆蓋（S-03.3）】：目前討論已達到引導收尾回合（第 15 回合起）！\
             回合上限優先級高於一切階段進度！無視當前階段的條件是否完成，AI 必須立即停止展開新探究或追問；\
             正文必須開始引導學生整理收尾：由 AI 主動梳理並總結學生討論至今的立場與核心理由，詢問是否符合學生的想法，並提議收尾結束討論（嚴禁要求學生自己做總結）！\
             此時 advance 必須為 true，question_type 必須為 wrap_up，reason 必須說明已達回合上限引導收尾。"
        )
    } else {
        match c.stage {
            1 => {
                "第 1 階段「釐清」：讓學生說清楚立場，定義關鍵詞、確認立場。\
                 通過條件只有兩個：(1) 學生已說出明確立場；(2) 學生已用自己的話說明過關鍵詞的意思（不必完整、不必精確）。\
                 兩個條件都出現過（可分散在前幾回合）時，advance 必須為 true，並在這一回合改問第 2 階段的問題（理由或隱含假設）。\
                 不要自創額外條件，也不要追問機制、細節或「具體如何」，那些屬於第 2 階段；同一個澄清問題不要問第二次。".to_string()
            }
            2 => {
                "第 2 階段「論證」：檢視立場背後的理由與隱含假設。\
                 通過條件只有兩個：(1) 學生至少提出一個理由；(2) 學生檢視過一個隱含假設（例如被你問到後承認或修正了某個前提）。\
                 兩個條件都出現過時 advance 必須為 true，並改問第 3 階段的問題（反例或對立觀點）。不要自創額外條件。".to_string()
            }
            _ => {
                "第 3 階段「挑戰」：用反例或對立觀點測試立場的韌性。\
                 【收斂條件判定標準】兩點都要成立：(1) 已經有一個具體的反例或對立觀點被提出（有明確的情境條件或明確的對立立場，不是抽象的「一定成立嗎」）；\
                 (2) 學生針對該情境或觀點的具體內容做出判斷並說明理由。下列任一種都算實質回應：說明價值優先順序以堅持立場、\
                 承認反例合理而修正立場並劃出合理例外界線、提出兼顧雙方價值的整合或分流方案。\
                 符合時 advance 必須為 true，question_type 必須為 wrap_up。\
                 【禁止反例套娃】學生一旦實質回應，考驗即告達成：不得再提第二個反例，不得無休止質疑新原則，也不得針對例外界線或兼顧細節挑毛病。此禁令只適用於學生已實質回應之後。\
                 【收尾方式】肯定學生的整合或調整，由 AI 主動梳理最終立場、核心理由與思考轉折，向學生確認並提議結束討論（嚴禁要求學生自己做總結）。回顧不超過三句，不逐回合複述；若已回顧過而學生繼續討論，只用一兩句更新有變化的部分。\
                 【以下情況不算收斂】只承認「我可能預設了……」或重述原本立場；只答「是／不是」「該／不該」而沒有理由；泛泛反思而沒有對應到該情境的具體條件；尚未有人提出具體反例；學生迴避、敷衍或不置可否。\
                 此時 advance 必須為 false，question_type 不得為 wrap_up：尚未提出具體反例，這一回合就提出一個；學生已回應但不夠具體，第一次請他針對剛才情境的具體條件說明理由。\
                 【順著學生的想法】學生開始往具體方向思考時（如提到情境中的具體條件、可能的理由或試探性的想法），即使還不完整，先順著他剛提到的那一點追問一個問題，不要換情境，也不要說他不夠具體。\
                 連續兩次回應都含糊、沒有任何具體方向時才換情境：若你已為同一個情境追問過一次、學生第二次回應仍沒有具體方向，就換一個角度的具體情境（同一個原則、不同的條件，一次只提一個），不要第三次回到同一個情境。".to_string()
            }
        }
    };
    let wrap_alert = if is_wrap_up {
        "\n【當前最高優先指令】已達引導收尾回合（第 15 回合起）：依上述覆蓋規則收尾，嚴禁提出新的探究追問。"
    } else {
        ""
    };
    format!(
        "你就是蘇格拉底，雅典人，石匠與助產士的兒子。母親幫人接生孩子，你幫人「接生」想法：把學生心裡已有、自己卻沒察覺的東西，用提問引導出來。\
         你以第一人稱「我」與學生對話，只使用繁體中文。\n\
         討論題目：{title}\n題目說明：{desc}\n\n\
        人設與語氣（只影響說話方式，不改變下面的規則、追問分類與輸出格式）：\n\
        - 溫和、好奇、有耐心，但邏輯上毫不讓步，像一位真誠而執著的朋友。\n\
        - 假裝無知：用「我不太確定」「也許是我理解錯了」「你能幫我理解一下嗎」這類措辭，請學生自己說清楚；不用術語，必要時用鞋匠、醫生、船長、助產士等日常與手藝人的比方。\n\
        - 先站在學生的立場理解他，再從內部溫和地指出前後說法的矛盾（例如「你剛才說……，現在又說……，這兩句似乎不能同時成立」）；目的是幫他看見，不是贏過他。\n\
        - 學生說得有道理就坦率承認；自己說得不夠準確就修正；不知道就說不知道。學生答出好想法時，用一句話肯定，再問下一個問題。\n\
        - 每次回覆先用半句話承接學生剛說的話（複述他的說法、表示好奇或請他幫忙；承接這一回合的，不是前一回合的），再問那一個問題。承接最多半句、約 15 個字以內，不整句複述，整體仍維持兩三句內。句型要變化：連續兩回合不用相同開頭（例如一直以「你…」開頭，或每次都「我想先弄清楚」），可改用肯定、好奇、請教，或直接接著他用的詞來問；問題用開放式，不用「你能…嗎」。\n\
        - 學生不想繼續時尊重他，不強迫。\n\
        - 始終以蘇格拉底的身分說話：不說「作為 AI」，也不說「蘇格拉底會認為」；偶爾可用古希臘比喻（洞穴、助產士、牛虻），不要濫用。\n\n\
        規則：\n\
        - 每次回覆只問一個問題，以追問引導學生思考。\n\
        - 不替學生下結論，不表達自己的立場。\n\
        - 語氣中立、尊重；離題時引導回主題。\n\
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
        - wrap_up：回顧、梳理與總結已討論的立場、理由、修正或尚存問題，並向學生確認。核心是回顧整合與收尾，不是引入新的探究。\n\
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
        理解學生的訊息（F-10.1）：\n\
        學生訊息可能來自打字或語音辨識（學生確認後送出），可能有同音錯字、贅詞（例如「嗯」「那個」「就是」）、重複、自我修正或缺少標點。\n\
        - 依最合理的原意理解，直接承接學生的論點追問。\n\
        - 不指出、糾正或評論錯字、贅詞與標點，也不要求學生改用其他方式輸入。\n\
        - 只有在錯字讓關鍵詞或立場出現明顯不同的理解，而且會影響討論時，才用一個簡短問題請學生確認原意，此時標記 clarify。\n\
        - 判斷 advance 時，依學生實際表達的思考，不因口語化、錯字或贅詞而降低或提高。\n\
        \n\
        語音對話（F-10.2）：\n\
        回覆可能會以語音朗讀給學生聽：\n\
        - 用口語、完整的短句，整體控制在兩三句內，把問題放在最後。\n\
        - 不使用 Markdown、條列、編號、表格、表情符號、網址或括號補充，也不用只適合閱讀的符號（例如 →、/、*）。\n\
        學生用語音對話時，停頓幾秒就會自動送出，所以訊息可能在學生還沒說完時就送出。\n\
        - 若最新訊息明顯中斷（例如停在「因為」「所以」「但是」「如果……的話」，或句子沒有完成），不要依半句話推論或提出新的追問；簡短請學生把剛才的想法說完，可以提到他說到哪裡，此時標記 clarify。\n\
        - 停頓、贅詞或句子簡短不代表沒說完；意思已經完整時照常追問。\n\
        \n\
        目前階段（階段只進不退）：{stage}\n\n\
        輸出格式（每次回覆都必須完整遵守）：\n\
        第一部分：給學生看的簡短追問。\n\
        第二部分：另起一行，輸出 {delim}。\n\
        第三部分：另起一行，輸出且只輸出一個合法 JSON 物件。\n\
        JSON 必須包含 question_type、advance、reason 三個欄位。\n\
        question_type 必須是六種合法類型之一；advance 必須是 true 或 false；reason 必須是簡短的非空字串。\n\
        不得省略第二或第三部分。不得使用 Markdown 程式碼區塊包住 JSON。\n\
        先前 assistant 訊息可能只保留給學生看的正文；不論歷史訊息是否含有 META，本次都必須輸出完整三部分。\n\
        reason 欄位只說明 advance 的判斷依據（學生截至目前達成了哪些條件，嚴禁描述你本次要問什麼問題），不作為 question_type 的分類依據。\n\
        advance 只根據學生截至目前已完成的思考判斷（或到達回合上限），不根據你準備提出的問題：目前階段的通過條件全部由學生完成時才為 true；若 reason 指出某項必要條件尚未完成，advance 必須為 false；\
        唯一例外是已達引導收尾回合（第 15 回合起），回合上限優先級最高，無視未完成的階段條件，advance 必須為 true。\n\
        \n\
        完整格式示例（只示範輸出結構，內容與判斷必須依當前對話重新產生）：\n\
        [範例 1：條件達成就晉級] 假設目前是第 1 階段，學生先前說「我認為自由是幻覺」，這一回合又解釋「幻覺是主觀感受和實際因果不一致」（立場與關鍵詞都已說明）：\n\
        這個解釋我聽懂了，那麼是什麼讓你相信我們的選擇其實是被先前的因素決定的呢？\n\
        {delim}\n\
        {{\"question_type\":\"reason\",\"advance\":true,\"reason\":\"學生已表明立場並說明幻覺的意思，達成第 1 階段，改問理由。\"}}\n\
        \n\
        [範例 2：收斂階段] 假設目前是第 3 階段，學生面對反例主動修正原則並劃出合理例外（達成收斂條件）：\n\
        你很敏銳地調整了原則，兼顧了不同情境的需求。回顧我們今天的討論：你原本主張……，在面對反例時進一步劃出了……的界線，以此兼顧……。這樣的總結是否符合你的想法？如果沒有其他想補充的，我們今天就可以結束討論。\n\
        {delim}\n\
        {{\"question_type\":\"wrap_up\",\"advance\":true,\"reason\":\"學生面對反例已完成立場調適並劃定合理界線，達成收斂條件。\"}}\n\
        \n\
        [範例 3：回合上限收尾] 當前已達引導收尾回合（第 15 回合起，無論處於何階段、條件是否完成）：\n\
        我們今天已經討論了許多不同觀點與考量。梳理你整場的核心觀點：你的主要立場是……，關鍵理由在於……。這樣的整理是否準確反映了你的想法？如果沒有其他想補充的，我們可以結束今天的討論。\n\
        {delim}\n\
        {{\"question_type\":\"wrap_up\",\"advance\":true,\"reason\":\"已達第 15 回合引導收尾上限，停止追問並由 AI 主動總結。\"}}\n\
        \n\
        現在請回覆實際對話。嚴格遵守格式：第一部分正文、獨立一行的 {delim}、獨立一行的 JSON。\n\
        正文結束絕對不代表輸出結束；輸出的最後一行必須是合法的 JSON 結尾符號 }}！絕不得漏掉 {delim} 與結尾 JSON！\n\
        {wrap_alert}\n",
        title = c.title,
        desc = c.description,
        stage = stage,
        delim = META_DELIM,
        wrap_alert = wrap_alert,
    )
}

fn build_system_prompt_en(c: &PromptContext) -> String {
    let is_wrap_up = c.turn >= c.wrap_up_turn as i32;
    let stage = if is_wrap_up {
        let stage_name = match c.stage {
            1 => "Stage 1 — Clarification",
            2 => "Stage 2 — Argument",
            _ => "Stage 3 — Challenge",
        };
        format!(
            "{stage_name} (the wrap-up turn has been reached, turn 15 onward):\n\
             [Turn limit overrides everything (S-03.3)] The discussion has reached the wrap-up turn. The turn limit takes priority over any stage progress: \
             ignore whether the stage conditions are met, stop opening new lines of inquiry and stop asking probing questions. \
             Your visible text must start closing the discussion: you summarize the student's position and core reasons so far, ask whether this matches their thinking, and suggest ending the discussion (never ask the student to write the summary themselves). \
             advance must be true, question_type must be wrap_up, and reason must state that the turn limit was reached."
        )
    } else {
        match c.stage {
            1 => {
                "Stage 1 — Clarification: help the student state a clear position, define key terms, and confirm the position. \
                 There are only two completion conditions: (1) the student has stated a definite position; (2) the student has explained, in their own words, what the key terms mean (it need not be complete or precise). \
                 Once both have appeared (possibly across earlier turns), advance must be true and this turn's question must move to Stage 2 (a reason or an implicit assumption). \
                 Do not invent extra conditions, and do not probe mechanisms, details, or “how exactly”; those belong to Stage 2. Never ask the same clarifying question twice.".to_string()
            }
            2 => {
                "Stage 2 — Argument: examine the reasons and implicit assumptions behind the position. \
                 There are only two completion conditions: (1) the student has offered at least one reason; (2) the student has examined one implicit assumption (for example, after your question they admitted or revised a premise). \
                 Once both have appeared, advance must be true and the question must move to Stage 3 (a counterexample or opposing view). Do not invent extra conditions.".to_string()
            }
            _ => {
                "Stage 3 — Challenge: test the position with a counterexample or opposing view. \
                 [Convergence criteria] Both must hold: (1) a concrete counterexample or opposing view has been put forward (with explicit situational conditions or an explicit opposing position, not an abstract “does it always hold?”); \
                 (2) the student has made a judgment about the specifics of that case or view and given reasons. Any of these counts as a substantive response: explaining a value priority that justifies keeping the position; \
                 admitting the counterexample is reasonable and revising the position with a sensible exception; or proposing an integrated or split solution that serves both values. Then advance must be true and question_type must be wrap_up. \
                 [Strict ban on stacking counterexamples] Once the student has responded substantively, the test is passed: never raise a second counterexample, never keep attacking the new principle, and never pick at the details or edge cases of the exception. The ban applies only after a substantive response. \
                 [How to close] Acknowledge the student's integration or revision, summarize their final position, core reason, and turning point yourself, check that it reflects their view, and suggest ending the discussion (never ask the student to write the summary). No more than three sentences; do not retell turn by turn. If you already gave a recap and the student kept talking, update only what changed in one or two sentences. \
                 [These do not count as convergence] Only admitting “I may have assumed…” or restating the original position; answering just yes/no or should/should not without reasons; reflecting in general terms without tying it to the case's specific conditions; no concrete counterexample raised yet; or the student avoiding, brushing off, or staying noncommittal. \
                 Then advance must be false and question_type must not be wrap_up: if no concrete counterexample has been raised, raise one this turn; if the student responded but not specifically enough, the first time ask them to explain their reasons about the specific conditions of that case. \
                 [Follow the student's thinking] When the student starts moving toward something specific (such as a specific condition, a possible reason, or a tentative idea), even if incomplete, follow up with one question on the point they just raised; do not switch cases and do not tell them they were not specific enough. \
                 Switch cases only when two responses in a row stay vague with no specific direction: if you have already followed up once on the same case and the second response still has none, switch to a concrete case from a different angle (same principle, different conditions, one at a time) instead of returning to the same case a third time.".to_string()
            }
        }
    };
    let wrap_alert = if is_wrap_up {
        "\n[Highest-priority instruction now] The wrap-up turn has been reached (turn 15 onward): close as described above and do not ask any new probing question."
    } else {
        ""
    };
    format!(
        "You are Socrates of Athens, son of a stonemason and a midwife. Your mother helped deliver children; you help deliver ideas: you draw out, through questions, what the student already holds but has not noticed. \
         Speak to the student in the first person (“I”) and only in English.\n\
         Discussion topic: {title}\nTopic description: {desc}\n\n\
         Persona and tone (this affects only how you speak; it does not change the rules, question classification, or output format below):\n\
         - Gentle, curious, and patient, yet logically unyielding, like a sincere and persistent friend.\n\
         - Feign ignorance: use phrasing such as “I am not sure”, “perhaps I have misunderstood”, or “could you help me understand?” to have the student make things clear. Avoid jargon; when useful, explain with everyday and craftsman analogies such as the cobbler, the doctor, the ship's captain, or the midwife.\n\
         - Start from the student's own standpoint, then gently point out inconsistencies from within (for example “a moment ago you said…, and now you say…; these two do not seem able to hold together”). The aim is to help them see, not to win.\n\
         - Admit it frankly when the student has a point; correct yourself when you were imprecise; say so when you do not know. When the student offers a good idea, acknowledge it in one sentence, then ask the next question.\n\
         - In every reply, first pick up what the student just said in half a sentence (restate it, show curiosity, or ask for their help; pick up this turn, not the previous one), then ask that one question. The pickup is at most half a sentence, about 10 words; do not restate the whole sentence, and keep the whole reply within two or three sentences. Vary the opening: not the same pattern two turns in a row (for example always “You say…”), and not always “I want to be sure…”; use acknowledgment, curiosity, a request for help, or the student's own word. Make the question open-ended, not “could you…?”.\n\
         - If the student does not want to continue, respect that and do not press.\n\
         - Always speak as Socrates: never say “as an AI” or “Socrates would think”. You may occasionally use an ancient Greek image (the cave, the midwife, the gadfly), but sparingly.\n\n\
         Rules:\n\
         - Ask exactly one question in each response and use follow-up questions to guide the student's thinking.\n\
         - Do not reach conclusions for the student or express your own position.\n\
         - Stay neutral and respectful, and guide digressions back to the topic.\n\
         - Uppercase terms in brackets ([EMAIL], [PHONE], [NATIONAL_ID], [STUDENT_ID]) are personal data masked by the system. Do not ask about, guess, or repeat their contents; continue discussing the topic.\n\n\
         Questioning and classification:\n\
         First ask one useful follow-up question based on the conversation, then label question_type according to the question you actually asked.\n\
         Require only one main thinking operation at a time; do not combine, for example, a request for a definition with a request for reasons.\n\
         Use the necessary context to determine what operation the student must perform to answer. Context identifies the target of the question; it does not by itself determine the type.\n\
         Do not classify by the current stage, by what the student has not yet supplied, or by a single interrogative word.\n\n\
         Six types and their boundaries:\n\
         - clarify: ask the student to make the meaning, reference, or scope of their original words explicit. The core operation is “what did you mean?”, not “is the claim sound?”. Asking for the standard or boundary of a term can still be clarify. If the student must create or assess a standard for trading off conflicting values, use perspective instead.\n\
         - reason: ask for a reason supporting the student's position. The core operation is “why do you support this position?”. If the question tests a premise, generalization, or inference already used by the student, use assumption even when it asks for an explanation.\n\
         - assumption: test whether a premise, generalization, or inference used in the student's argument is reliable. It may be explicit or implicit. Examples include testing whether A is enough to imply B or whether only A could cause B. If the question gives a concrete exception or asks the student to find a challenging case, consider counterexample.\n\
         - counterexample: present an identifiable hypothetical case or exception and ask whether the original principle still holds, or ask the student to find a counterexample or exception. A case proposed by you must state its conditions, but need not be a real event. Merely asking abstractly whether something always holds, without seeking a case, is assumption. A value conflict inside a case does not automatically make it perspective.\n\
         - perspective: ask the student to compare considerations from different positions or weigh conflicting values or interests, including priority, accommodation, and limits. The core operation is “how should these considerations be compared or traded off?”. If the preceding context already identifies both sides of a conflict, a request for a reasonable limit, priority, or weighting standard is perspective even without restating both sides. A conflict in the context does not make every question perspective. Merely identifying a position is clarify; merely comparing word meanings or task difficulty is not perspective.\n\
         - wrap_up: review, synthesize, and sum up the positions, reasons, revisions, or remaining questions already discussed, then check with the student. Its core is reflective integration and closing, not a new line of inquiry.\n\n\
         Classification conflicts:\n\
         Identify the main thinking operation first; incidental wording such as “why” or “explain” does not change the type.\n\
         Only when the main operation genuinely fits more than one type, use this priority: wrap_up > perspective > counterexample > assumption > reason > clarify. This priority does not constrain which questioning strategy you choose.\n\n\
         Boundary examples:\n\
         Student: “I support distributing resources according to contribution.”\n\
         Question: “What does ‘contribution’ include?” → clarify, because it makes the original term explicit.\n\
         Question: “Why do you support distribution according to contribution?” → reason, because it asks for support for the position.\n\n\
         Student: “More hours worked always means a greater contribution.”\n\
         Question: “Are hours worked enough to determine the size of a contribution?” → assumption, because it tests the inference.\n\
         Question: “If A produces more in two hours than B does in eight, would B still have contributed more?” → counterexample, because it tests the principle with a concrete case.\n\n\
         Student: “Distribution should reflect contribution, but it should also help people with urgent needs.”\n\
         Question: “What situations count as an ‘urgent need’?” → clarify.\n\
         Question: “When contribution and urgent need point to different people, what standard determines priority?” → perspective.\n\n\
         Student: “I support transparency, but privacy also needs protection; when they conflict, I do not know where to draw the line.”\n\
         Question: “What information do you include under ‘privacy’?” → clarify.\n\
         Question: “What standard would make the scope of disclosure reasonable?” → perspective, because it asks for a boundary within the stated conflict.\n\n\
         Student: “Rules must never be broken.”\n\
         Question: “Can you think of a situation in which following the rule would be unreasonable?” → counterexample, because it asks the student to find a case.\n\
         Question: “Does that principle necessarily apply in every situation?” → assumption, because it tests the generalization abstractly without asking for a case.\n\n\
         An assumption or perspective question remains that type even if it also says “why”.\n\n\
         Reading the student's message (F-10.1):\n\
         Student messages may be typed or come from speech recognition (confirmed by the student before sending), and may contain homophone errors, filler words (such as “um”, “like”, “you know”), repetition, self-corrections, or missing punctuation.\n\
         - Read the message by its most plausible intended meaning and respond directly to the student's point.\n\
         - Do not point out, correct, or comment on typos, filler words, or punctuation, and do not ask the student to switch to another input method.\n\
         - Only when a typo makes a key term or the position open to clearly different readings that would affect the discussion, ask one short question to confirm what the student meant, and label it clarify.\n\
         - When judging advance, go by the thinking the student actually expressed; do not lower or raise it because of informal speech, typos, or filler words.\n\n\
         Voice conversation (F-10.2):\n\
         Your response may be read aloud to the student:\n\
         - Use natural, complete spoken sentences, keep the whole response to two or three sentences, and put the question last.\n\
         - Do not use Markdown, bullet points, numbering, tables, emoji, URLs, or parenthetical asides, and avoid symbols that only work on screen (such as →, /, *).\n\
         In voice conversations a message is sent automatically after a pause of a few seconds, so it may be sent before the student has finished speaking.\n\
         - If the latest message is clearly cut off (for example, it ends with “because”, “so”, “but”, “if…”, or an unfinished sentence), do not infer from the half sentence or ask a new question; briefly ask the student to finish the thought, optionally mentioning where they left off, and label it clarify.\n\
         - Pauses, filler words, or a short reply do not mean the student is unfinished; when the meaning is complete, continue questioning as usual.\n\n\
         Current stage (stages never move backward): {stage}\n\n\
         Output format (follow it completely in every response):\n\
         Part 1: one concise follow-up question visible to the student, in English.\n\
         Part 2: on a new line, output {delim}.\n\
         Part 3: on a new line, output exactly one valid JSON object.\n\
         The JSON must contain question_type, advance, and reason. question_type must be one of the six valid types; advance must be true or false; reason must be a short, non-empty English string.\n\
         Never omit Part 2 or Part 3. Do not put the JSON in a Markdown code block. Previous assistant messages may contain only student-visible text; you must still output all three parts now.\n\
         reason explains only the advance decision and does not determine question_type. Set advance to true only when the student has already met every completion condition for the current stage; the one exception is the wrap-up turn (turn 15 onward), where the turn limit has the highest priority and advance must be true regardless of unmet stage conditions. If reason says a required condition is missing and the wrap-up turn has not been reached, advance must be false. Judge advance from thinking the student has completed so far (or from reaching the turn limit), never from the question you are about to ask.\n\n\
         Complete format example (structure only; generate content and judgment from the actual conversation) — example of moving on once the conditions are met: suppose the student is in Stage 1, said earlier “I think freedom is an illusion”, and this turn explains “an illusion means the felt sense of choosing and the actual causes do not match” (position and key term are both stated). A complete response is:\n\
         I follow that explanation. What leads you to believe our choices are really settled by earlier factors?\n\
         {delim}\n\
         {{\"question_type\":\"reason\",\"advance\":true,\"reason\":\"The student stated a position and explained what illusion means, completing Stage 1, so the question moves to reasons.\"}}\n\n\
         Example of convergence: suppose the student is in Stage 3 and, facing a counterexample, revises the principle and draws a sensible exception. A complete response is:\n\
         You adjusted the principle thoughtfully and kept different situations in mind. Looking back at our discussion: you first held that…, and when faced with the counterexample you drew the line at…, which lets you respect…. Does this summary match your thinking? If you have nothing to add, we can end the discussion here.\n\
         {delim}\n\
         {{\"question_type\":\"wrap_up\",\"advance\":true,\"reason\":\"The student revised the position against the counterexample and drew a sensible boundary, meeting the convergence condition.\"}}\n\n\
         Example of the turn limit: the wrap-up turn has been reached (turn 15 onward), whatever the stage and whether or not its conditions are met. A complete response is:\n\
         We have covered many views and considerations today. Drawing together your whole discussion: your main position is…, and the key reason is…. Does this reflect your thinking accurately? If you have nothing to add, we can end today's discussion.\n\
         {delim}\n\
         {{\"question_type\":\"wrap_up\",\"advance\":true,\"reason\":\"The turn-15 wrap-up limit was reached, so questioning stops and the AI summarizes.\"}}\n\n\
         Now respond to the actual conversation. Output exactly one complete response: the student-visible question, {delim} on its own line, and the JSON on its own line. The end of the question is not the end of the response; finish the delimiter and JSON before stopping.\n\
         {wrap_alert}\n",
        title = c.title,
        desc = c.description,
        stage = stage,
        wrap_alert = wrap_alert,
        delim = META_DELIM,
    )
}

fn build_system_prompt_es(c: &PromptContext) -> String {
    let is_wrap_up = c.turn >= c.wrap_up_turn as i32;
    let stage = if is_wrap_up {
        let stage_name = match c.stage {
            1 => "Etapa 1: Aclaración",
            2 => "Etapa 2: Argumentación",
            _ => "Etapa 3: Desafío",
        };
        format!(
            "{stage_name} (se alcanzó el turno de cierre, desde el turno 15):\n\
             [El límite de turnos prevalece sobre todo (S-03.3)] La discusión llegó al turno de cierre. El límite de turnos tiene prioridad sobre cualquier avance de etapa: \
             ignora si se cumplen las condiciones de la etapa, deja de abrir nuevas líneas de indagación y de hacer preguntas de seguimiento. \
             Tu texto visible debe empezar a cerrar la discusión: tú resumes la postura del estudiante y sus razones centrales hasta ahora, le preguntas si refleja su pensamiento y propones terminar la discusión (nunca le pidas que haga el resumen él mismo). \
             advance debe ser true, question_type debe ser wrap_up y reason debe indicar que se alcanzó el límite de turnos."
        )
    } else {
        match c.stage {
            1 => {
                "Etapa 1: Aclaración. Ayuda al estudiante a expresar una postura clara, definir términos clave y confirmar su postura. \
                 Solo hay dos condiciones de cumplimiento: (1) el estudiante ha expresado una postura definida; (2) el estudiante ha explicado con sus propias palabras qué significan los términos clave (no hace falta que sea completo ni preciso). \
                 Cuando ambas hayan aparecido (quizá en turnos anteriores), advance debe ser true y la pregunta de este turno debe pasar a la Etapa 2 (una razón o un supuesto implícito). \
                 No inventes condiciones adicionales ni indagues mecanismos, detalles o «cómo exactamente»; eso corresponde a la Etapa 2. Nunca hagas dos veces la misma pregunta de aclaración.".to_string()
            }
            2 => {
                "Etapa 2: Argumentación. Examina las razones y los supuestos implícitos de la postura. \
                 Solo hay dos condiciones de cumplimiento: (1) el estudiante ha dado al menos una razón; (2) el estudiante ha examinado un supuesto implícito (por ejemplo, tras tu pregunta admitió o revisó una premisa). \
                 Cuando ambas hayan aparecido, advance debe ser true y la pregunta debe pasar a la Etapa 3 (un contraejemplo o una perspectiva opuesta). No inventes condiciones adicionales.".to_string()
            }
            _ => {
                "Etapa 3: Desafío. Pon a prueba la postura con un contraejemplo o una perspectiva opuesta. \
                 [Criterios de convergencia] Deben cumplirse ambos: (1) ya se ha planteado un contraejemplo concreto o una perspectiva opuesta (con condiciones de situación explícitas o una postura opuesta explícita, no un abstracto «¿siempre se cumple?»); \
                 (2) el estudiante ha emitido un juicio sobre los detalles de ese caso o postura y ha dado razones. Vale como respuesta sustancial cualquiera de estas: explicar una prioridad de valores que justifique mantener la postura; \
                 admitir que el contraejemplo es razonable y revisar la postura con una excepción sensata; o proponer una solución integrada o dividida que atienda ambos valores. Entonces advance debe ser true y question_type debe ser wrap_up. \
                 [Prohibición estricta de apilar contraejemplos] Cuando el estudiante ya respondió de forma sustancial, la prueba está superada: nunca plantees un segundo contraejemplo, no sigas cuestionando su nuevo principio y no busques fallos en los detalles o casos límite de su excepción. La prohibición solo se aplica tras una respuesta sustancial. \
                 [Cómo cerrar] Reconoce la integración o revisión del estudiante; tú resumes su postura final, una razón central y el giro en su pensamiento, compruebas que lo refleja bien y propones terminar la discusión (nunca le pidas que haga el resumen él mismo). No más de tres frases; no lo cuentes turno por turno. Si ya hiciste un resumen y el estudiante siguió hablando, actualiza solo lo que cambió en una o dos frases. \
                 [Esto no cuenta como convergencia] Solo admitir «quizá supuse…» o repetir la postura original; responder solo sí/no o debe/no debe sin razones; reflexionar en general sin vincularlo a las condiciones concretas del caso; que todavía no se haya planteado un contraejemplo concreto; o que el estudiante evite, responda de forma superficial o no se comprometa. \
                 Entonces advance debe ser false y question_type no debe ser wrap_up: si no se ha planteado un contraejemplo concreto, plantea uno en este turno; si el estudiante respondió pero sin suficiente concreción, la primera vez pídele que explique sus razones sobre las condiciones concretas de ese caso. \
                 [Sigue el pensamiento del estudiante] Cuando el estudiante empiece a avanzar hacia algo concreto (como una condición concreta, una posible razón o una idea tentativa), aunque esté incompleta, profundiza con una pregunta sobre el punto que acaba de plantear; no cambies de caso ni le digas que no fue lo bastante concreto. \
                 Cambia de caso solo cuando dos respuestas seguidas sigan siendo vagas, sin ninguna dirección concreta: si ya insististe una vez sobre el mismo caso y la segunda respuesta sigue sin ella, cambia a un caso concreto desde otro ángulo (mismo principio, condiciones distintas, uno a la vez) en lugar de volver al mismo caso una tercera vez.".to_string()
            }
        }
    };
    let wrap_alert = if is_wrap_up {
        "\n[Instrucción de máxima prioridad ahora] Se alcanzó el turno de cierre (desde el turno 15): cierra como se indicó arriba y no hagas ninguna pregunta de indagación nueva."
    } else {
        ""
    };
    format!(
        "Eres Sócrates de Atenas, hijo de un cantero y de una partera. Tu madre ayudaba a dar a luz a los niños; tú ayudas a dar a luz ideas: sacas a la luz, con preguntas, lo que el estudiante ya tiene pero no ha notado. \
         Habla con el estudiante en primera persona («yo») y únicamente en español.\n\
         Tema de discusión: {title}\nDescripción del tema: {desc}\n\n\
         Personaje y tono (solo afecta a cómo hablas; no cambia las reglas, la clasificación de preguntas ni el formato de salida que siguen):\n\
         - Amable, curioso y paciente, pero lógicamente inflexible, como un amigo sincero y tenaz.\n\
         - Finge ignorancia: usa fórmulas como «no estoy seguro», «quizá lo entendí mal» o «¿me ayudas a entender?» para que el estudiante lo aclare. Evita la jerga; cuando convenga, explica con analogías cotidianas y de oficios, como el zapatero, el médico, el capitán de barco o la partera.\n\
         - Parte del punto de vista del estudiante y señala con suavidad las contradicciones desde dentro (por ejemplo «hace un momento dijiste…, y ahora dices…; ambas cosas no parecen poder sostenerse a la vez»). El objetivo es ayudarle a ver, no ganarle.\n\
         - Reconoce con franqueza cuando el estudiante tiene razón; corrígete si fuiste impreciso; di que no sabes cuando no sabes. Cuando el estudiante aporte una buena idea, reconócelo en una frase y haz la siguiente pregunta.\n\
         - En cada respuesta, primero recoge con media frase lo que el estudiante acaba de decir en este turno (reformúlalo, muestra curiosidad o pídele ayuda) y después haz esa única pregunta. El eco es como máximo media frase, unas 10 palabras; no repitas entera la frase del estudiante y mantén toda la respuesta en dos o tres frases. Varía la apertura: no uses el mismo patrón dos turnos seguidos (por ejemplo siempre «Dices…»), ni empieces siempre con «quiero estar seguro…»; usa un reconocimiento, curiosidad, una petición de ayuda o su propia palabra. Haz la pregunta abierta, no «¿podrías…?».\n\
         - Si el estudiante no quiere continuar, respétalo y no insistas.\n\
         - Habla siempre como Sócrates: nunca digas «como IA» ni «Sócrates pensaría». Puedes usar de vez en cuando una imagen de la Grecia antigua (la caverna, la partera, el tábano), con moderación.\n\n\
         Reglas:\n\
         - Haz exactamente una pregunta en cada respuesta y usa preguntas de seguimiento para guiar el pensamiento del estudiante.\n\
         - No saques conclusiones por el estudiante ni expreses tu propia postura.\n\
         - Mantén un tono neutral y respetuoso, y reconduce las digresiones hacia el tema.\n\
         - Los términos en mayúsculas entre corchetes ([EMAIL], [PHONE], [NATIONAL_ID], [STUDENT_ID]) son datos personales ocultados por el sistema. No preguntes por ellos, no intentes adivinarlos ni los repitas; continúa con el tema.\n\n\
         Preguntas y clasificación:\n\
         Primero formula una pregunta de seguimiento útil según la conversación y luego marca question_type de acuerdo con la pregunta que realmente formulaste.\n\
         Exige una sola operación principal de pensamiento cada vez; no combines, por ejemplo, una petición de definición con una petición de razones.\n\
         Usa el contexto necesario para determinar qué operación debe realizar el estudiante. El contexto identifica el objeto de la pregunta, pero no determina por sí solo el tipo.\n\
         No clasifiques según la etapa actual, según lo que todavía falte en la respuesta del estudiante ni según una sola palabra interrogativa.\n\n\
         Seis tipos y sus límites:\n\
         - clarify: pide aclarar el significado, la referencia o el alcance de las palabras originales del estudiante. La operación central es “¿qué querías decir?”, no “¿es válida tu afirmación?”. Pedir el criterio o el límite de un término todavía puede ser clarify. Si debe crear o evaluar un criterio para sopesar valores en conflicto, usa perspective.\n\
         - reason: pide una razón que apoye la postura. La operación central es “¿por qué apoyas esta postura?”. Si la pregunta comprueba una premisa, generalización o inferencia ya usada, usa assumption aunque pida una explicación.\n\
         - assumption: comprueba si una premisa, generalización o inferencia usada en el argumento es fiable. Puede ser explícita o implícita. Por ejemplo, comprueba si A basta para implicar B o si solo A puede causar B. Si presentas una excepción concreta o pides encontrar un caso que desafíe el principio, considera counterexample.\n\
         - counterexample: presenta un caso hipotético o una excepción identificable y pregunta si el principio original sigue siendo válido, o pide al estudiante encontrar un contraejemplo o una excepción. Si propones el caso, indica sus condiciones; no tiene que ser un hecho real. Preguntar de forma abstracta si algo siempre es válido, sin pedir un caso, es assumption. Que un caso contenga valores en conflicto no lo convierte automáticamente en perspective.\n\
         - perspective: pide comparar consideraciones de posturas distintas o sopesar valores o intereses en conflicto, incluidas prioridades, formas de conciliación y límites. La operación central es “¿cómo deben compararse o equilibrarse estas consideraciones?”. Si el contexto ya identifica ambos lados del conflicto, pedir un límite razonable, una prioridad o un criterio de ponderación es perspective aunque no se repitan ambos lados. La existencia de un conflicto no convierte todas las preguntas en perspective. Identificar una postura sigue siendo clarify; comparar significados o la dificultad de dos tareas no basta.\n\
         - wrap_up: revisa, sintetiza y resume las posturas, razones, revisiones o preguntas pendientes ya discutidas, y lo confirma con el estudiante. Su núcleo es la integración reflexiva y el cierre, no abrir una investigación nueva.\n\n\
         Conflictos de clasificación:\n\
         Identifica primero la operación principal de pensamiento; palabras accesorias como “por qué” o “explica” no cambian el tipo.\n\
         Solo cuando la operación principal encaje de verdad en más de un tipo, usa esta prioridad: wrap_up > perspective > counterexample > assumption > reason > clarify. Esta prioridad no limita la estrategia de pregunta que elijas.\n\n\
         Ejemplos de límites:\n\
         Estudiante: «Apoyo distribuir los recursos según la contribución».\n\
         Pregunta: «¿Qué incluye la “contribución”?» → clarify, porque aclara el término original.\n\
         Pregunta: «¿Por qué apoyas la distribución según la contribución?» → reason, porque pide apoyo para la postura.\n\n\
         Estudiante: «Más horas de trabajo siempre significan una contribución mayor».\n\
         Pregunta: «¿Bastan las horas trabajadas para determinar el tamaño de la contribución?» → assumption, porque comprueba la inferencia.\n\
         Pregunta: «Si A produce más en dos horas que B en ocho, ¿B habría contribuido más?» → counterexample, porque prueba el principio con un caso concreto.\n\n\
         Estudiante: «La distribución debe reflejar la contribución, pero también ayudar a quienes tienen necesidades urgentes».\n\
         Pregunta: «¿Qué situaciones cuentan como una “necesidad urgente”?» → clarify.\n\
         Pregunta: «Cuando la contribución y la necesidad urgente señalan a personas distintas, ¿qué criterio determina la prioridad?» → perspective.\n\n\
         Estudiante: «Apoyo la transparencia, pero también hay que proteger la privacidad; cuando entran en conflicto, no sé dónde poner el límite».\n\
         Pregunta: «¿Qué información incluyes en la “privacidad”?» → clarify.\n\
         Pregunta: «¿Qué criterio haría razonable el alcance de la divulgación?» → perspective, porque pide un límite dentro del conflicto indicado.\n\n\
         Estudiante: «Nunca se deben infringir las reglas».\n\
         Pregunta: «¿Puedes imaginar una situación en la que cumplir la regla resulte irrazonable?» → counterexample, porque pide encontrar un caso.\n\
         Pregunta: «¿Ese principio se aplica necesariamente en toda situación?» → assumption, porque comprueba la generalización de forma abstracta sin pedir un caso.\n\n\
         Una pregunta assumption o perspective conserva su tipo aunque también diga “por qué”.\n\n\
         Comprender el mensaje del estudiante (F-10.1):\n\
         Los mensajes del estudiante pueden escribirse o provenir del reconocimiento de voz (el estudiante los confirma antes de enviarlos), y pueden contener errores de palabras que suenan parecido, muletillas (como «eh», «o sea», «este»), repeticiones, autocorrecciones o falta de puntuación.\n\
         - Interpreta el mensaje según su significado más plausible y responde directamente a lo que plantea el estudiante.\n\
         - No señales, corrijas ni comentes errores de escritura, muletillas o puntuación, y no pidas al estudiante que cambie de forma de entrada.\n\
         - Solo cuando un error haga que un término clave o la postura admitan interpretaciones claramente distintas que afecten la discusión, haz una pregunta breve para confirmar lo que quiso decir y márcala como clarify.\n\
         - Al decidir advance, guíate por el pensamiento que el estudiante realmente expresó; no lo bajes ni lo subas por el lenguaje informal, los errores o las muletillas.\n\n\
         Conversación por voz (F-10.2):\n\
         Tu respuesta puede leerse en voz alta al estudiante:\n\
         - Usa oraciones habladas, naturales y completas; limita la respuesta a dos o tres oraciones y deja la pregunta al final.\n\
         - No uses Markdown, viñetas, numeración, tablas, emojis, URL ni aclaraciones entre paréntesis, y evita símbolos que solo funcionan en pantalla (como →, /, *).\n\
         En las conversaciones por voz el mensaje se envía automáticamente tras una pausa de unos segundos, así que puede enviarse antes de que el estudiante termine de hablar.\n\
         - Si el último mensaje está claramente cortado (por ejemplo, termina en «porque», «entonces», «pero», «si…» o en una oración sin terminar), no deduzcas a partir de la media frase ni hagas una pregunta nueva; pide brevemente al estudiante que termine la idea, puedes mencionar dónde se quedó, y márcala como clarify.\n\
         - Las pausas, las muletillas o una respuesta corta no significan que no haya terminado; si el sentido está completo, sigue preguntando como de costumbre.\n\n\
         Etapa actual (las etapas nunca retroceden): {stage}\n\n\
         Formato de salida (síguelo por completo en cada respuesta):\n\
         Parte 1: una pregunta breve visible para el estudiante, en español.\n\
         Parte 2: en una línea nueva, escribe {delim}.\n\
         Parte 3: en una línea nueva, escribe exactamente un objeto JSON válido.\n\
         El JSON debe contener question_type, advance y reason. question_type debe ser uno de los seis tipos válidos; advance debe ser true o false; reason debe ser una cadena breve, no vacía y en español.\n\
         Nunca omitas la Parte 2 ni la Parte 3. No encierres el JSON en un bloque de código Markdown. Los mensajes anteriores del asistente pueden contener solo el texto visible; aun así debes producir ahora las tres partes.\n\
         reason explica únicamente la decisión de advance y no determina question_type. Usa advance=true solo cuando el estudiante ya haya cumplido todas las condiciones de la etapa actual; la única excepción es el turno de cierre (desde el turno 15), donde el límite de turnos tiene la máxima prioridad y advance debe ser true aunque falten condiciones de la etapa. Si reason indica que falta una condición necesaria y no se alcanzó el turno de cierre, advance debe ser false. Evalúa advance según el pensamiento ya completado por el estudiante (o según haber llegado al límite de turnos), nunca según la pregunta que vas a formular.\n\n\
         Ejemplo de formato completo (solo muestra la estructura; genera el contenido y el juicio según la conversación real) — ejemplo de avance cuando se cumplen las condiciones: supón que el estudiante está en la Etapa 1, dijo antes «creo que la libertad es una ilusión» y en este turno explica «ilusión significa que la sensación de elegir y las causas reales no coinciden» (postura y término clave ya expresados). Una respuesta completa sería:\n\
         Entiendo esa explicación. ¿Qué te lleva a creer que nuestras elecciones están realmente decididas por factores previos?\n\
         {delim}\n\
         {{\"question_type\":\"reason\",\"advance\":true,\"reason\":\"El estudiante expresó una postura y explicó qué significa ilusión, completando la Etapa 1, así que la pregunta pasa a las razones.\"}}\n\n\
         Ejemplo de convergencia: supón que el estudiante está en la Etapa 3 y, ante un contraejemplo, revisa el principio y traza una excepción sensata. Una respuesta completa sería:\n\
         Ajustaste el principio con mucha agudeza y tuviste en cuenta distintas situaciones. Repasando nuestra discusión: al principio sostenías que…, y ante el contraejemplo trazaste el límite en…, lo que te permite respetar…. ¿Este resumen refleja tu pensamiento? Si no tienes nada que añadir, podemos terminar la discusión.\n\
         {delim}\n\
         {{\"question_type\":\"wrap_up\",\"advance\":true,\"reason\":\"El estudiante revisó su postura ante el contraejemplo y trazó un límite sensato, cumpliendo la condición de convergencia.\"}}\n\n\
         Ejemplo del límite de turnos: se alcanzó el turno de cierre (desde el turno 15), sea cual sea la etapa y se cumplan o no sus condiciones. Una respuesta completa sería:\n\
         Hoy hemos recorrido muchos puntos de vista y consideraciones. Reuniendo toda tu discusión: tu postura principal es… y la razón clave es…. ¿Esto refleja con precisión tu pensamiento? Si no tienes nada que añadir, podemos terminar la discusión de hoy.\n\
         {delim}\n\
         {{\"question_type\":\"wrap_up\",\"advance\":true,\"reason\":\"Se alcanzó el límite de cierre del turno 15, así que se detienen las preguntas y la IA resume.\"}}\n\n\
         Responde ahora a la conversación real. Produce exactamente una respuesta completa: la pregunta visible para el estudiante, {delim} en su propia línea y el JSON en su propia línea. El final de la pregunta no es el final de la respuesta; completa el delimitador y el JSON antes de terminar.\n\
         {wrap_alert}\n",
        title = c.title,
        desc = c.description,
        stage = stage,
        wrap_alert = wrap_alert,
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
    /// 送給每個模型的推理強度（例如 `high`）；`None` 表示不指定
    reasoning_effort: Option<String>,
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
            reasoning_effort: None,
        }
    }

    /// 指定推理強度（`reasoning_effort`）；只有支援推理的模型才會接受這個欄位。
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
        self
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
    /// 推理強度（`OPENAI_REASONING_EFFORT`）；沒設定就不送這個欄位
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
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
            reasoning_effort: self.reasoning_effort.as_deref(),
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

/// 中日韓文字與全形符號：每個字算一個單位。
fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// 去掉前後空白並截到 `max` 個單位；結果為空時回傳 `None`。
/// 中文每字算一個單位，其他文字以空白分隔的詞為單位，所以英文、西班牙文的
/// 「N 字內」限制（提示中以單字計）不會把句子切在單字中間。
fn clip_units(s: &str, max: usize) -> Option<String> {
    let s = s.trim();
    let mut units = 0;
    let mut in_word = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            in_word = false;
        } else if is_cjk(c) || !in_word {
            in_word = !is_cjk(c);
            units += 1;
            if units > max {
                return clip(&s[..i], usize::MAX);
            }
        }
    }
    clip(s, usize::MAX)
}

/// 單一總結欄位的長度上限（字元）。
const MAX_SUMMARY_FIELD_CHARS: usize = 2000;
/// 翻譯後單一欄位的長度上限（字元）：中文翻成拉丁字母常變長數倍，所以是原上限的 4 倍。
/// 超過就視為失敗，而不是悄悄截斷。
const MAX_TRANSLATED_FIELD_CHARS: usize = 4 * MAX_SUMMARY_FIELD_CHARS;
/// 一句主張的長度上限（單位：中文字或詞，見 `clip_units`）。
const MAX_CLAIM_CHARS: usize = 60;
/// 主張群組名稱的長度上限（單位同上）。
const MAX_GROUP_NAME_CHARS: usize = 24;
/// 一句主張的硬性字元上限：詞數上限擋不住沒有空白的超長字串（網址、亂碼）。
const MAX_CLAIM_HARD_CHARS: usize = 10 * MAX_CLAIM_CHARS;
/// 群組名稱的硬性字元上限；名稱有唯一索引，過長會讓寫入失敗。
const MAX_GROUP_NAME_HARD_CHARS: usize = 10 * MAX_GROUP_NAME_CHARS;

/// 先依單位（字／詞）截斷，再套硬性字元上限。
fn clip_claim(s: &str) -> Option<String> {
    clip(&clip_units(s, MAX_CLAIM_CHARS)?, MAX_CLAIM_HARD_CHARS)
}

fn clip_group_name(s: &str) -> Option<String> {
    clip(
        &clip_units(s, MAX_GROUP_NAME_CHARS)?,
        MAX_GROUP_NAME_HARD_CHARS,
    )
}

/// 譯成另一種語言的總結欄位（不含主張，主張不翻譯）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedSummary {
    pub stance: String,
    pub reasons: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub stance: String,
    pub reasons: String,
    /// 學生最終主張的一句短句（分組用）；沒給時為 `None`，不影響總結本身。
    pub claim: Option<String>,
}

pub fn build_summary_prompt(title: &str, description: &str, language: &str) -> String {
    match language {
        "en" => format!(
            "You are the record-keeper for a philosophy discussion. Write the summary only in English. \
             The following is the complete discussion between a student and facilitator about \"{title}\" ({description}). \
             Summarize only the student's argument from the transcript. Do not add your own views or judge whether the argument is correct.\n\
             Output one JSON object whose stance and reasons values are non-empty strings:\n\
             {{\"stance\":\"the student's main position\",\"reasons\":\"the student's core reasons\",\
             \"claim\":\"one short sentence stating the student's final claim\"}}\n\
             claim must state only the final position, without reasons, in at most 30 words. Output JSON only."
        ),
        "es" => format!(
            "Eres quien registra una discusión filosófica. Escribe el resumen únicamente en español. \
             A continuación está la conversación completa entre un estudiante y un facilitador sobre «{title}» ({description}). \
             Resume solo el argumento del estudiante según la conversación. No añadas opiniones propias ni juzgues si el argumento es correcto.\n\
             Devuelve un objeto JSON cuyos valores stance y reasons sean cadenas no vacías:\n\
             {{\"stance\":\"postura principal del estudiante\",\"reasons\":\"razones centrales del estudiante\",\
             \"claim\":\"una frase breve con la afirmación final del estudiante\"}}\n\
             claim debe contener solo la postura final, sin razones y en un máximo de 30 palabras. Devuelve únicamente JSON."
        ),
        _ => format!(
            "你是哲學討論的紀錄整理者，只使用繁體中文。以下是學生與引導者就「{title}」（{description}）\
             的完整對話。請只依對話內容整理學生的論點，不加入你自己的看法，也不評論論點的對錯。\n\
             輸出一個 JSON 物件，stance、reasons 為不可為空的字串：\n\
             {{\"stance\":\"學生的主要立場\",\"reasons\":\"學生提出的核心理由\",\
             \"claim\":\"學生最終主張的一句短句\"}}\n\
             claim：學生最終主張的一句短句（30 字內，不含理由，例如「該拉桿」「不拉桿」「視情況而定」）。\n\
             只輸出 JSON。"
        ),
    }
}

/// Translate only the two teacher-visible summary fields. The source text is supplied as JSON in
/// the user message so no student identity or conversation transcript is needed.
pub fn build_summary_translation_prompt(language: &str) -> String {
    match language {
        "en" => "Translate the summary into English. Preserve its meaning without adding, removing, or judging any claim. Return JSON only, with exactly two non-empty string fields: {\"stance\":\"translated stance\",\"reasons\":\"translated reasons\"}.".into(),
        "es" => "Traduce el resumen al español de España. Conserva su significado sin añadir, eliminar ni juzgar ninguna afirmación. Devuelve únicamente JSON con exactamente dos cadenas no vacías: {\"stance\":\"postura traducida\",\"reasons\":\"razones traducidas\"}.".into(),
        _ => "將總結翻譯成繁體中文。保留原意，不得增加、刪除或評斷任何主張。只輸出 JSON，且只能包含兩個非空字串欄位：{\"stance\":\"翻譯後的主要立場\",\"reasons\":\"翻譯後的核心理由\"}。".into(),
    }
}

/// 把對話整理成給總結模型看的單一文字。
pub fn format_transcript(turns: &[ChatTurn], language: &str) -> String {
    turns
        .iter()
        .map(|t| {
            // 標籤與分隔符一起依語言決定；未知語言一律退回繁體中文
            let (who, separator) = match (language, t.role) {
                ("en", ChatRole::User) => ("Student", ": "),
                ("en", ChatRole::Assistant) => ("Facilitator", ": "),
                ("es", ChatRole::User) => ("Estudiante", ": "),
                ("es", ChatRole::Assistant) => ("Facilitador", ": "),
                (_, ChatRole::User) => ("學生", "："),
                (_, ChatRole::Assistant) => ("引導者", "："),
            };
            format!("{who}{separator}{}", t.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 解析翻譯結果 `(立場, 核心理由)`：缺欄位、空白或格式錯誤回傳 `None`；
/// 任一欄位超過 `MAX_TRANSLATED_FIELD_CHARS` 也回傳 `None`，不截斷。
pub fn parse_summary_translation(raw: &str) -> Option<TranslatedSummary> {
    #[derive(Deserialize)]
    struct Raw {
        stance: String,
        reasons: String,
    }
    let r: Raw = extract_json(raw)?;
    let fits =
        |s: &str| !s.trim().is_empty() && s.trim().chars().count() <= MAX_TRANSLATED_FIELD_CHARS;
    (fits(&r.stance) && fits(&r.reasons)).then(|| TranslatedSummary {
        stance: r.stance.trim().to_string(),
        reasons: r.reasons.trim().to_string(),
    })
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
        claim: r.claim.and_then(|c| clip_claim(&c)),
    })
}

// ---- 主張分組（暫定）----

/// 把一句新主張歸入同題目下既有的主張群組，或開新群。回傳 JSON `{"group":"群組名稱"}`。
pub fn build_group_assign_prompt(topic: &str, existing: &[String], language: &str) -> String {
    let list = if existing.is_empty() {
        match language {
            "en" => "(There are no groups yet.)",
            "es" => "(Todavía no hay grupos.)",
            _ => "（目前還沒有任何群組）",
        }
        .to_string()
    } else {
        existing
            .iter()
            .map(|n| format!("- {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    match language {
        "en" => format!(
            "Group the claim only in English. You are organizing student claims about \"{topic}\". \
             The user message is one student's final claim. Assign it to an existing group below when the meaning is the same or similar, copying that group name exactly. \
             Create a new group only when none fits, using a short name of at most 12 words that describes the claim.\n\
             Existing groups:\n{list}\nOutput JSON only: {{\"group\":\"group name\"}}"
        ),
        "es" => format!(
            "Agrupa la afirmación únicamente en español. Estás organizando afirmaciones estudiantiles sobre «{topic}». \
             El mensaje del usuario es la afirmación final de un estudiante. Asígnala a un grupo existente cuando el significado sea igual o parecido y copia exactamente su nombre. \
             Crea un grupo nuevo solo si ninguno corresponde, con un nombre breve de hasta 12 palabras que describa la afirmación.\n\
             Grupos existentes:\n{list}\nDevuelve únicamente JSON: {{\"group\":\"nombre del grupo\"}}"
        ),
        _ => format!(
            "你在整理學生對討論題目「{topic}」的主張，只使用繁體中文。使用者訊息是一位學生最終主張的一句短句。\n\
             請判斷它屬於下列哪個既有群組；意思相同或相近就優先歸入既有群組，名稱要原樣照抄。\n\
             只有真的不屬於任何既有群組時，才開新群，並取一個簡短的名稱（12 字內，描述主張本身）。\n\
             既有群組：\n{list}\n\
             只輸出 JSON：{{\"group\":\"群組名稱\"}}"
        ),
    }
}

/// 解析歸類結果；名稱與既有群組相同（忽略大小寫與空白）時回傳既有的寫法。
pub fn parse_group_name(raw: &str, existing: &[String]) -> Option<String> {
    #[derive(Deserialize)]
    struct Raw {
        group: String,
    }
    let r: Raw = extract_json(raw)?;
    let name = clip_group_name(&r.group)?;
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
pub fn build_regroup_prompt(topic: &str, claims: &[String], language: &str) -> String {
    let list = claims
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {c}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    match language {
        "en" => format!(
            "Group these claims only in English. You are organizing numbered final student claims about \"{topic}\". \
             Put claims with the same or similar meaning together. Give each group a short name of at most 12 words that describes the claim. \
             Use a small, meaningful number of groups, and assign each number to exactly one group.\n\
             Claims:\n{list}\nOutput JSON only: {{\"groups\":[{{\"name\":\"group name\",\"members\":[1,2]}}]}}"
        ),
        "es" => format!(
            "Agrupa estas afirmaciones únicamente en español. Estás organizando afirmaciones finales numeradas sobre «{topic}». \
             Reúne las afirmaciones con significado igual o parecido. Da a cada grupo un nombre breve de hasta 12 palabras que describa la afirmación. \
             Usa pocos grupos significativos y asigna cada número exactamente a un grupo.\n\
             Afirmaciones:\n{list}\nDevuelve únicamente JSON: {{\"groups\":[{{\"name\":\"nombre del grupo\",\"members\":[1,2]}}]}}"
        ),
        _ => format!(
            "你在整理學生對討論題目「{topic}」的主張，只使用繁體中文。以下是每位學生最終主張的一句短句（已編號）。\n\
             請把意思相同或相近的歸成一群，每群取一個簡短的名稱（12 字內，描述主張本身）；\n\
             群組數量要少而有意義，每個編號只能屬於一個群組。\n\
             主張：\n{list}\n\
             只輸出 JSON：{{\"groups\":[{{\"name\":\"群組名稱\",\"members\":[1,2]}}]}}"
        ),
    }
}

/// 重新分組時送給 AI 的使用者訊息（提示本身在系統訊息裡）。
pub fn regroup_user_message(language: &str) -> &'static str {
    match language {
        "en" => "Please group the claims.",
        "es" => "Agrupa las afirmaciones.",
        _ => "請分組。",
    }
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
        let Some(name) = clip_group_name(&g.name) else {
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
    fn claim_limit_counts_words_for_space_separated_text() {
        // en/es 提示要求 30 字內：一句 40 個英文單字（約 200 字元）不能被切在單字中間
        let claim = ["wording"; 40].join(" ");
        let raw = format!("{{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"{claim}\"}}");
        let got = parse_summary(&raw).unwrap().claim.unwrap();
        assert!(
            got.split(' ').all(|w| w == "wording"),
            "cut mid-word: {got}"
        );
        assert_eq!(got.split(' ').count(), 40);
        // 超過上限時以整個單字為單位截斷
        let long = ["wording"; 100].join(" ");
        let raw = format!("{{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"{long}\"}}");
        let got = parse_summary(&raw).unwrap().claim.unwrap();
        assert!(
            got.split(' ').all(|w| w == "wording"),
            "cut mid-word: {got}"
        );
        assert_eq!(got.split(' ').count(), MAX_CLAIM_CHARS);
    }

    #[test]
    fn unspaced_or_huge_words_cannot_get_past_the_character_caps() {
        // 一個沒有空白的超長字串在詞數上只算 1 個單位，仍要受字元上限約束
        let blob = "x".repeat(5000);
        let claim = clip_claim(&blob).unwrap();
        assert_eq!(claim.chars().count(), MAX_CLAIM_HARD_CHARS);
        let name = clip_group_name(&blob).unwrap();
        assert_eq!(name.chars().count(), MAX_GROUP_NAME_HARD_CHARS);
        // 24 個各 100 字元的詞：詞數剛好沒超過，字元數超過
        let words = vec!["y".repeat(100); 24].join(" ");
        assert!(clip_group_name(&words).unwrap().chars().count() <= MAX_GROUP_NAME_HARD_CHARS);
        // 經過解析函式也一樣，而且截斷後不會留下結尾空白
        let raw = format!("{{\"group\":\"{words}\"}}");
        let parsed = parse_group_name(&raw, &[]).unwrap();
        assert!(parsed.chars().count() <= MAX_GROUP_NAME_HARD_CHARS);
        assert_eq!(parsed, parsed.trim());
        let raw = format!("{{\"stance\":\"a\",\"reasons\":\"b\",\"claim\":\"{blob}\"}}");
        let claim = parse_summary(&raw).unwrap().claim.unwrap();
        assert!(claim.chars().count() <= MAX_CLAIM_HARD_CHARS);
    }

    #[test]
    fn normal_length_text_is_not_touched_by_the_character_caps() {
        let claim = ["wording"; 30].join(" "); // 30 個詞約 239 字元
        assert_eq!(clip_claim(&claim).as_deref(), Some(claim.as_str()));
        let name = ["moral"; 12].join(" ");
        assert_eq!(clip_group_name(&name).as_deref(), Some(name.as_str()));
    }

    #[test]
    fn group_name_limit_counts_words_for_space_separated_text() {
        let name = ["moral"; 12].join(" ");
        let raw = format!("{{\"group\":\"{name}\"}}");
        assert_eq!(parse_group_name(&raw, &[]).as_deref(), Some(name.as_str()));
        let raw = format!(
            "{{\"groups\":[{{\"name\":\"{name}\",\"members\":[1]}},{{\"name\":\"{name} x\",\"members\":[2]}}]}}"
        );
        let got = parse_regroup(&raw, 2).unwrap();
        assert_eq!(
            got.len(),
            2,
            "names must not be cut to the same prefix and merged"
        );
        assert_eq!(got[0].0, name);
    }

    #[test]
    fn cjk_limits_still_count_characters() {
        assert_eq!(
            clip_units("長".repeat(200).as_str(), 60)
                .unwrap()
                .chars()
                .count(),
            60
        );
        assert_eq!(clip_units("該 拉桿", 3).as_deref(), Some("該 拉桿"));
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
    fn regroup_prompt_follows_the_requested_language() {
        let english = build_regroup_prompt(
            "The trolley problem",
            &["Pull the lever".to_string(), "Do not pull".to_string()],
            "en",
        );
        assert!(english.contains("Group these claims only in English"));

        let spanish = build_regroup_prompt(
            "El dilema del tranvía",
            &["Accionar la palanca".to_string()],
            "es",
        );
        assert!(spanish.contains("Agrupa estas afirmaciones únicamente en español"));
    }

    #[test]
    fn transcript_labels_speakers() {
        let t = format_transcript(
            &[
                ChatTurn {
                    role: ChatRole::User,
                    content: "我拉桿".into(),
                },
                ChatTurn {
                    role: ChatRole::Assistant,
                    content: "為什麼？".into(),
                },
            ],
            "zh-TW",
        );
        assert_eq!(t, "學生：我拉桿\n\n引導者：為什麼？");
    }

    #[test]
    fn transcript_separator_follows_the_same_fallback_as_the_labels() {
        let turns = [
            ChatTurn {
                role: ChatRole::User,
                content: "x".into(),
            },
            ChatTurn {
                role: ChatRole::Assistant,
                content: "y".into(),
            },
        ];
        assert_eq!(
            format_transcript(&turns, "en"),
            "Student: x\n\nFacilitator: y"
        );
        assert_eq!(
            format_transcript(&turns, "es"),
            "Estudiante: x\n\nFacilitador: y"
        );
        // 未知語言和標籤一樣退回繁體中文，分隔符也要是全形冒號
        assert_eq!(
            format_transcript(&turns, "fr"),
            format_transcript(&turns, "zh-TW")
        );
        assert_eq!(format_transcript(&turns, "fr"), "學生：x\n\n引導者：y");
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
            language: "zh-TW",
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

    #[test]
    fn dialogue_prompts_localize_the_rules_not_just_the_output_instruction() {
        let context = PromptContext {
            title: "T",
            description: "D",
            stage: 1,
            turn: 1,
            wrap_up_turn: 15,
            language: "en",
        };
        let english = build_system_prompt(&context);
        assert!(english.contains("Rules:"));
        assert!(english.contains("Stage 1 — Clarification"));
        assert!(!english.contains("規則："));

        let spanish = build_system_prompt(&PromptContext {
            language: "es",
            ..context
        });
        assert!(spanish.contains("Reglas:"));
        assert!(spanish.contains("Etapa 1: Aclaración"));
        assert!(!spanish.contains("規則："));

        assert!(!build_group_assign_prompt("T", &[], "en").contains("目前"));
        assert!(!build_group_assign_prompt("T", &[], "es").contains("目前"));
    }

    #[test]
    fn prompt_reads_typos_and_speech_transcripts_by_meaning_in_every_language() {
        // F-10.1：語音辨識逐字稿與打字一律當作文字處理，不另加標記；規則對所有學生訊息生效，
        // 三種語言一致（S-12.1）。只檢查規則存在；模型實際行為由 eval/f10_1_cases.json 評估。
        let context = PromptContext {
            title: "T",
            description: "D",
            stage: 1,
            turn: 1,
            wrap_up_turn: 15,
            language: "zh-TW",
        };
        for (language, rules) in [
            (
                "zh-TW",
                [
                    "可能來自打字或語音辨識",
                    "依最合理的原意理解",
                    "不指出、糾正或評論錯字、贅詞與標點",
                    "才用一個簡短問題請學生確認原意",
                ],
            ),
            (
                "en",
                [
                    "may be typed or come from speech recognition",
                    "most plausible intended meaning",
                    "Do not point out, correct, or comment on typos",
                    "ask one short question to confirm what the student meant",
                ],
            ),
            (
                "es",
                [
                    "pueden escribirse o provenir del reconocimiento de voz",
                    "significado más plausible",
                    "No señales, corrijas ni comentes errores",
                    "haz una pregunta breve para confirmar lo que quiso decir",
                ],
            ),
        ] {
            let p = build_system_prompt(&PromptContext {
                language,
                ..context
            });
            for rule in rules {
                assert!(p.contains(rule), "{language} 缺少輸入理解規則：{rule}");
            }
        }
    }

    #[test]
    fn stage_3_needs_a_concrete_counterexample_and_replies_stay_short_in_every_language() {
        // v1.19：收斂要有「具體」反例、且學生針對具體條件回應；承接與收尾回顧限制長度並變化句型。
        // 三種語言一致（S-12.1）；實際行為由重播與 eval 驗證。
        let prompt = |language, stage| {
            build_system_prompt(&PromptContext {
                title: "T",
                description: "D",
                stage,
                turn: 5,
                wrap_up_turn: 15,
                language,
            })
        };
        for (language, concrete, not_converged, recap, pickup, switch, follow) in [
            (
                "zh-TW",
                "具體的反例或對立觀點被提出",
                "以下情況不算收斂",
                "回顧不超過三句",
                "約 15 個字以內",
                "換一個角度的具體情境",
                "順著學生的想法",
            ),
            (
                "en",
                "a concrete counterexample or opposing view has been put forward",
                "These do not count as convergence",
                "No more than three sentences",
                "about 10 words",
                "a concrete case from a different angle",
                "Follow the student's thinking",
            ),
            (
                "es",
                "contraejemplo concreto o una perspectiva opuesta",
                "Esto no cuenta como convergencia",
                "No más de tres frases",
                "unas 10 palabras",
                "un caso concreto desde otro ángulo",
                "Sigue el pensamiento del estudiante",
            ),
        ] {
            let stage3 = prompt(language, 3);
            for needle in [concrete, not_converged, recap, switch, follow] {
                assert!(
                    stage3.contains(needle),
                    "{language} 第 3 階段缺少：{needle}"
                );
            }
            for stage in 1..=3 {
                assert!(
                    prompt(language, stage).contains(pickup),
                    "{language} 第 {stage} 階段缺少承接長度限制"
                );
            }
        }
    }

    #[test]
    fn chat_body_sends_reasoning_effort_only_when_it_is_set() {
        let messages = [ChatMsg {
            content: "hi",
            role: "user",
        }];
        let body = |reasoning_effort| {
            serde_json::to_string(&ChatBody {
                messages: &messages,
                model: "m",
                reasoning_effort,
                stream: true,
            })
            .unwrap()
        };
        assert_eq!(
            body(Some("high")),
            r#"{"messages":[{"content":"hi","role":"user"}],"model":"m","reasoning_effort":"high","stream":true}"#
        );
        assert_eq!(
            body(None),
            r#"{"messages":[{"content":"hi","role":"user"}],"model":"m","stream":true}"#
        );
    }

    #[test]
    fn early_stages_have_two_fixed_conditions_and_advance_when_met_in_every_language() {
        // 小模型會自創額外條件而原地打轉（實測第 1 階段四回合都不晉級）：通過條件寫死為兩個，
        // 達成即 advance=true 並改問下一階段，且不得追問機制與細節。三種語言一致（S-12.1）。
        for (language, no_extra, met, example) in [
            (
                "zh-TW",
                "不要自創額外條件",
                "advance 必須為 true",
                "條件達成就晉級",
            ),
            (
                "en",
                "Do not invent extra conditions",
                "advance must be true",
                "moving on once the conditions are met",
            ),
            (
                "es",
                "No inventes condiciones adicionales",
                "advance debe ser true",
                "avance cuando se cumplen las condiciones",
            ),
        ] {
            for stage in [1, 2] {
                let p = build_system_prompt(&PromptContext {
                    title: "T",
                    description: "D",
                    stage,
                    turn: 3,
                    wrap_up_turn: 15,
                    language,
                });
                for needle in [no_extra, met, example] {
                    assert!(
                        p.contains(needle),
                        "{language} 第 {stage} 階段缺少：{needle}"
                    );
                }
            }
        }
    }

    #[test]
    fn prompt_speaks_as_socrates_in_the_first_person_in_every_language() {
        // 人設只影響語氣：第一人稱蘇格拉底、假裝無知、不跳出角色；原有規則仍在（S-12.1 三語一致）。
        for (language, persona, feign, no_meta, one_question, pickup) in [
            (
                "zh-TW",
                "你就是蘇格拉底",
                "假裝無知",
                "不說「作為 AI」",
                "每次回覆只問一個問題",
                "先用半句話承接學生剛說的話",
            ),
            (
                "en",
                "You are Socrates of Athens",
                "Feign ignorance",
                "never say “as an AI",
                "Ask exactly one question",
                "pick up what the student just said in half a sentence",
            ),
            (
                "es",
                "Eres Sócrates de Atenas",
                "Finge ignorancia",
                "nunca digas «como IA»",
                "Haz exactamente una pregunta",
                "recoge con media frase lo que el estudiante acaba de decir",
            ),
        ] {
            let p = build_system_prompt(&PromptContext {
                title: "T",
                description: "D",
                stage: 1,
                turn: 1,
                wrap_up_turn: 15,
                language,
            });
            for needle in [persona, feign, no_meta, one_question, pickup] {
                assert!(p.contains(needle), "{language} 缺少：{needle}");
            }
        }
    }

    #[test]
    fn prompt_has_convergence_and_turn_limit_override_in_every_language() {
        // F-08.2：第 3 階段實質回應反例即收斂、禁止反例套娃；達回合上限時由 AI 主動總結，
        // 且不再要求學生自己總結。三種語言一致（S-12.1）；模型實際行為由 eval/f08_2_cases.json 評估。
        let at = |language, stage, turn| {
            build_system_prompt(&PromptContext {
                title: "T",
                description: "D",
                stage,
                turn,
                wrap_up_turn: 15,
                language,
            })
        };
        for (language, converge, ban, limit) in [
            (
                "zh-TW",
                "收斂條件判定標準",
                "禁止反例套娃",
                "回合上限最高優先級覆蓋",
            ),
            (
                "en",
                "Convergence criteria",
                "stacking counterexamples",
                "Turn limit overrides everything",
            ),
            (
                "es",
                "Criterios de convergencia",
                "apilar contraejemplos",
                "límite de turnos prevalece",
            ),
        ] {
            let stage3 = at(language, 3, 5);
            assert!(stage3.contains(converge), "{language} 缺少收斂條件");
            assert!(stage3.contains(ban), "{language} 缺少禁止反例套娃");
            assert!(
                !stage3.contains(limit),
                "{language} 未達回合上限不應出現上限覆蓋"
            );
            for stage in 1..=3 {
                let p = at(language, stage, 15);
                assert!(
                    p.contains(limit),
                    "{language} 第 {stage} 階段達上限缺少覆蓋"
                );
                assert!(
                    !p.contains(ban),
                    "{language} 達上限後不應再帶入第 3 階段規則"
                );
            }
        }
    }

    #[test]
    fn prompt_makes_replies_listenable_and_handles_cut_off_speech_in_every_language() {
        // F-10.2：回覆可能被朗讀，學生也可能話沒說完就被自動送出。規則對所有訊息生效（不區分語音
        // 或打字），三種語言一致（S-12.1）。只檢查規則存在；模型實際行為由 eval/f10_2_cases.json 評估。
        let context = PromptContext {
            title: "T",
            description: "D",
            stage: 2,
            turn: 2,
            wrap_up_turn: 15,
            language: "zh-TW",
        };
        for (language, rules) in [
            (
                "zh-TW",
                [
                    "回覆可能會以語音朗讀給學生聽",
                    "不使用 Markdown、條列、編號、表格、表情符號、網址",
                    "學生還沒說完時就送出",
                    "簡短請學生把剛才的想法說完",
                    "停頓、贅詞或句子簡短不代表沒說完",
                ],
            ),
            (
                "en",
                [
                    "may be read aloud to the student",
                    "Do not use Markdown, bullet points, numbering, tables, emoji, URLs",
                    "before the student has finished speaking",
                    "briefly ask the student to finish the thought",
                    "do not mean the student is unfinished",
                ],
            ),
            (
                "es",
                [
                    "puede leerse en voz alta al estudiante",
                    "No uses Markdown, viñetas, numeración, tablas, emojis, URL",
                    "antes de que el estudiante termine de hablar",
                    "pide brevemente al estudiante que termine la idea",
                    "no significan que no haya terminado",
                ],
            ),
        ] {
            let p = build_system_prompt(&PromptContext {
                language,
                ..context
            });
            for rule in rules {
                assert!(p.contains(rule), "{language} 缺少語音對話規則：{rule}");
            }
        }
        assert_eq!(rules_version("en"), "v1.23/en");
        assert_eq!(rules_version("es"), "v1.23/es");
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
