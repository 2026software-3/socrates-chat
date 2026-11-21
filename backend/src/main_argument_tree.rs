//! 以「一個主論點 → 延伸的子論點」重現原始 HTML v2 的閱讀樹。
//!
//! 與完整 Argument Mining graph 是不同的資料契約；不使用舊版 nodes+edges 產樹。
//! 只供本機授權驗證，尚未註冊 HTTP route、DB 或權限機制。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{config::load_openai_keys, pii::mask};

pub const PROMPT_VERSION: &str = "main-argument-tree-precision-v2.1.1";
pub const SYSTEM_PROMPT: &str = include_str!("../eval/main_argument_tree/prompt.md");
pub const OUTPUT_SCHEMA: &str = include_str!("../eval/main_argument_tree/output_schema.json");
const MAX_NODES: usize = 150;
const MAX_RAW_BYTES: usize = 1_000_000;

/// 逐字稿：獨立於 Argument Mining，從最初的 socrates-chat.zip 即可使用。
/// 保留未知輸入欄位，讓其他逐字稿 metadata 不會在遮蔽時消失。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub topic_id: String,
    pub motion: String,
    pub rounds: Vec<TranscriptRound>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptRound {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub ai_question: String,
    pub user_answer: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Transcript {
    pub fn validate(&self) -> Result<(), TreeError> {
        if self.topic_id.trim().is_empty() || self.motion.trim().is_empty() {
            return Err(TreeError::Invalid("topic_id / motion 不得為空".into()));
        }
        if !(1..=100).contains(&self.rounds.len()) {
            return Err(TreeError::Invalid("逐字稿需有 1 至 100 輪".into()));
        }
        for (i, round) in self.rounds.iter().enumerate() {
            let expected = format!("{}-R{:02}", self.topic_id, i + 1);
            if round.round.is_some_and(|number| number != i + 1)
                || round.id.as_ref().is_some_and(|id| id != &expected)
                || (i > 0 && round.ai_question.trim().is_empty())
                || round.user_answer.trim().is_empty()
            {
                return Err(TreeError::Invalid(format!(
                    "第 {} 筆輪次 ID 不正確或問答為空",
                    i + 1
                )));
            }
        }
        Ok(())
    }
}

/// Accept the legacy rounds format or the current flattened chat/detail response.
/// Preserve array order: a user message answers the preceding assistant, never the following one.
pub fn parse_transcript(raw: &str) -> Result<Transcript, TreeError> {
    let value: Value = serde_json::from_str(raw).map_err(TreeError::Json)?;
    if value.get("rounds").is_some() {
        if value.get("messages").is_some() {
            return Err(TreeError::Invalid(
                "輸入不能同時包含 rounds 與 messages".into(),
            ));
        }
        let input: Transcript = serde_json::from_value(value).map_err(TreeError::Json)?;
        input.validate()?;
        return Ok(input);
    }
    let required_text = |keys: &[&str]| -> Result<String, TreeError> {
        keys.iter()
            .find_map(|key| value[*key].as_str().filter(|s| !s.trim().is_empty()))
            .map(str::to_owned)
            .ok_or_else(|| TreeError::Invalid(format!("聊天輸入缺少 {}", keys.join(" / "))))
    };
    let topic_id = required_text(&["topic_id", "conversation_id", "id"])?;
    let mut motion = required_text(&["title"])?;
    if let Some(description) = value["description"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
    {
        motion.push_str("\n\n");
        motion.push_str(description);
    }
    let messages = value["messages"]
        .as_array()
        .ok_or_else(|| TreeError::Invalid("聊天輸入缺少 messages 陣列".into()))?;
    let mut rounds: Vec<TranscriptRound> = Vec::new();
    let mut question = String::new();
    let mut question_type: Option<String> = None;
    let mut last_user = false;
    for (message_index, message) in messages.iter().enumerate() {
        let content = message["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| TreeError::Invalid("訊息 content 不可為空".into()))?;
        match message["role"].as_str() {
            Some("ai" | "assistant") => {
                if !question.is_empty() {
                    question.push('\n');
                }
                question.push_str(content);
                question_type = message["question_type"].as_str().map(str::to_owned);
                last_user = false;
            }
            Some("student" | "user") => {
                if last_user && let Some(previous) = rounds.last_mut() {
                    previous.user_answer.push('\n');
                    previous.user_answer.push_str(content);
                    if let Some(Value::Array(indices)) =
                        previous.extra.get_mut("source_message_indices")
                    {
                        indices.push(serde_json::json!(message_index + 1));
                    }
                    continue;
                }
                let mut extra = BTreeMap::new();
                extra.insert(
                    "source_message_indices".into(),
                    serde_json::json!([message_index + 1]),
                );
                if let Some(strategy) = question_type.take() {
                    extra.insert("question_type".into(), Value::String(strategy));
                }
                rounds.push(TranscriptRound {
                    round: Some(rounds.len() + 1),
                    id: None,
                    ai_question: std::mem::take(&mut question),
                    user_answer: content.into(),
                    extra,
                });
                last_user = true;
            }
            _ => {
                return Err(TreeError::Invalid(
                    "只接受 student / ai（或 user / assistant）訊息".into(),
                ));
            }
        }
    }
    // Only topic text and dialogue are needed; never forward names, account IDs or summaries.
    let result = Transcript {
        topic_id,
        motion,
        rounds,
        extra: BTreeMap::new(),
    };
    result.validate()?;
    Ok(result)
}

/// 遮蔽解析後的文字值，讓 JSON escaping 完全由 serde_json 負責。
/// 不改結構識別碼；驗證與模型使用同一份遮蔽後的文字。
pub fn sanitized_transcript(input: &Transcript) -> Result<Transcript, TreeError> {
    input.validate()?;
    let mut result = input.clone();
    result.motion = mask(std::mem::take(&mut result.motion));
    for value in result.extra.values_mut() {
        mask_metadata(value);
    }
    for round in &mut result.rounds {
        round.ai_question = mask(std::mem::take(&mut round.ai_question));
        round.user_answer = mask(std::mem::take(&mut round.user_answer));
        for value in round.extra.values_mut() {
            mask_metadata(value);
        }
    }
    result.validate()?;
    Ok(result)
}

fn mask_metadata(value: &mut Value) {
    match value {
        Value::String(text) => *text = mask(std::mem::take(text)),
        Value::Array(items) => items.iter_mut().for_each(mask_metadata),
        Value::Object(fields) => fields.values_mut().for_each(mask_metadata),
        _ => {}
    }
}

#[derive(Debug)]
pub enum TreeError {
    Invalid(String),
    Api(String),
    Json(serde_json::Error),
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "tree validation: {s}"),
            Self::Api(s) => write!(f, "tree API: {s}"),
            Self::Json(e) => write!(f, "tree JSON: {e}"),
        }
    }
}
impl std::error::Error for TreeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Claim,
    Premise,
    Boundary,
    Rebuttal,
    Precision,
    IndependentJudgment,
    OpposingProposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    User,
    AssistantQuestionScenario,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Support,
    Attack,
    Qualify,
    Precision,
    Reflect,
    ContextOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub source_round: usize,
    pub evidence: String,
    pub parent: Option<String>,
    pub relation: Option<Relation>,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outline {
    pub topic_id: String,
    pub title: String,
    pub nodes: Vec<OutlineNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayNode {
    #[serde(flatten)]
    pub node: OutlineNode,
    pub children: Vec<DisplayNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayTree {
    pub topic_id: String,
    pub title: String,
    pub node_count: usize,
    pub review_status: String,
    pub root: DisplayNode,
}

pub fn parse_and_build(input: &Transcript, raw: &str) -> Result<(Outline, DisplayTree), TreeError> {
    let outline: Outline = serde_json::from_str(raw).map_err(TreeError::Json)?;
    let tree = validate_and_build(input, &outline)?;
    Ok((outline, tree))
}

/// 結構與逐字引文驗證。不會將「關聯」當成「支持」的語意正確性保證。
/// 第 15 輪未收束不是錯誤；IndependentJudgment 可以是葉節點。
pub fn validate_and_build(input: &Transcript, outline: &Outline) -> Result<DisplayTree, TreeError> {
    input.validate()?;
    if outline.topic_id != input.topic_id {
        return Err(TreeError::Invalid("topic_id 與輸入不一致".into()));
    }
    if outline.title.trim().is_empty() {
        return Err(TreeError::Invalid("title 為空".into()));
    }
    if outline.nodes.is_empty() || outline.nodes.len() > MAX_NODES {
        return Err(TreeError::Invalid(format!(
            "節點數需介於 1 與 {MAX_NODES} 之間"
        )));
    }

    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let prefix = format!("{}-", input.topic_id);
    for (i, node) in outline.nodes.iter().enumerate() {
        if !node.id.starts_with(&prefix)
            || node.id.trim().is_empty()
            || node.label.trim().is_empty()
            || by_id.insert(node.id.as_str(), i).is_some()
        {
            return Err(TreeError::Invalid(format!(
                "節點 ID 或 label 不合法：{}",
                node.id
            )));
        }
        if node.source_round == 0 || node.source_round > input.rounds.len() {
            return Err(TreeError::Invalid(format!(
                "{} 引用了不存在的輪次",
                node.id
            )));
        }
        let round = &input.rounds[node.source_round - 1];
        let original = match (node.origin, node.kind) {
            (Origin::User, NodeKind::OpposingProposition)
            | (
                Origin::AssistantQuestionScenario,
                NodeKind::Claim
                | NodeKind::Premise
                | NodeKind::Boundary
                | NodeKind::Rebuttal
                | NodeKind::Precision
                | NodeKind::IndependentJudgment,
            ) => {
                return Err(TreeError::Invalid(format!(
                    "{} 將 AI 情境與使用者主張混淆",
                    node.id
                )));
            }
            (Origin::User, _) => &round.user_answer,
            (Origin::AssistantQuestionScenario, NodeKind::OpposingProposition) => {
                &round.ai_question
            }
        };
        if node.evidence.is_empty() || !original.contains(&node.evidence) {
            return Err(TreeError::Invalid(format!(
                "{} 原文引文不在對應輪次",
                node.id
            )));
        }
    }

    let root_id = format!("{}-C0", input.topic_id);
    let roots: Vec<usize> = outline
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.parent.is_none().then_some(i))
        .collect();
    if roots.len() != 1 {
        return Err(TreeError::Invalid("主論點必須恰好只有一個根節點".into()));
    }
    let root_idx = roots[0];
    let root = &outline.nodes[root_idx];
    if root.id != root_id
        || root.kind != NodeKind::Claim
        || root.origin != Origin::User
        || root.relation.is_some()
    {
        return Err(TreeError::Invalid(
            "根節點須為使用者 Claim，ID 為 topic_id-C0，且 relation=null".into(),
        ));
    }

    let mut children: Vec<Vec<usize>> = vec![vec![]; outline.nodes.len()];
    for (i, node) in outline.nodes.iter().enumerate() {
        let Some(parent_id) = node.parent.as_deref() else {
            continue;
        };
        let parent_idx = by_id.get(parent_id).copied().ok_or_else(|| {
            TreeError::Invalid(format!("{} 指向不存在的 parent {}", node.id, parent_id))
        })?;
        if i == parent_idx || node.relation.is_none() {
            return Err(TreeError::Invalid(format!(
                "{} parent 自指或缺少 relation",
                node.id
            )));
        }
        // 類別與關係對應，避免「解釋」被當支持、「遲疑」被當證據。
        // 這是結構約束；是否真的構成支持/反駁，仍需人工審核語意。
        let expected = match node.kind {
            NodeKind::Claim | NodeKind::Premise => Relation::Support,
            NodeKind::Boundary => Relation::Qualify,
            NodeKind::Rebuttal => Relation::Attack,
            NodeKind::Precision => Relation::Precision,
            NodeKind::IndependentJudgment => Relation::Reflect,
            NodeKind::OpposingProposition => Relation::ContextOnly,
        };
        if node.relation != Some(expected) {
            return Err(TreeError::Invalid(format!(
                "{} kind={:?} 應使用 relation={:?}，實際={:?}",
                node.id, node.kind, expected, node.relation
            )));
        }
        // 同輪、同類型的判斷接成父子通常是重複敘述，而非獨立的推理階層。
        let parent_node = &outline.nodes[parent_idx];
        if node.kind == NodeKind::IndependentJudgment
            && parent_node.kind == NodeKind::IndependentJudgment
            && node.source_round == parent_node.source_round
        {
            return Err(TreeError::Invalid(format!(
                "{} 與其父節點為同輪重複的 IndependentJudgment，請合併或分支",
                node.id
            )));
        }
        children[parent_idx].push(i);
    }

    // 即使恰好只有一根，也要檢查不存在脫離根的循環。
    for node in &outline.nodes {
        let mut seen = HashSet::new();
        let mut cursor = node.id.as_str();
        loop {
            if !seen.insert(cursor) {
                return Err(TreeError::Invalid(format!("{} 上方存在循環", node.id)));
            }
            let current = &outline.nodes[by_id[cursor]];
            match current.parent.as_deref() {
                Some(parent) => cursor = parent,
                None if cursor == root_id => break,
                None => return Err(TreeError::Invalid(format!("{} 沒有連到主根", node.id))),
            }
        }
    }

    fn expand(idx: usize, nodes: &[OutlineNode], children: &[Vec<usize>]) -> DisplayNode {
        DisplayNode {
            node: nodes[idx].clone(),
            children: children[idx]
                .iter()
                .map(|&child| expand(child, nodes, children))
                .collect(),
        }
    }

    Ok(DisplayTree {
        topic_id: outline.topic_id.clone(),
        title: outline.title.clone(),
        node_count: outline.nodes.len(),
        review_status: "unreviewed".into(),
        root: expand(root_idx, &outline.nodes, &children),
    })
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_node(node: &DisplayNode, depth: usize, out: &mut String) {
    let relationship = if depth == 0 {
        "主論點"
    } else {
        match node.node.relation {
            Some(Relation::Support) => "支持",
            Some(Relation::Attack) => "反駁",
            Some(Relation::Qualify) => "限制／條件修正",
            Some(Relation::Precision) => "精確化／補充",
            Some(Relation::Reflect) => "個人判斷／未定",
            Some(Relation::ContextOnly) => "AI 問句情境",
            None => "主論點",
        }
    };
    let mut text = String::new();
    text.push_str("<span class=tag>");
    text.push_str(&escape_html(relationship));
    text.push_str("</span><strong>");
    text.push_str(&escape_html(&node.node.label));
    text.push_str("</strong><span class=round>R");
    text.push_str(&node.node.source_round.to_string());
    text.push_str("</span>");
    let open = if depth <= 1 { " open" } else { "" };
    out.push_str("<li><details");
    out.push_str(open);
    out.push_str("><summary>");
    out.push_str(&text);
    out.push_str("</summary><p class=quote>引用：「");
    out.push_str(&escape_html(&node.node.evidence));
    out.push_str("」</p>");
    if !node.children.is_empty() {
        out.push_str("<ul>");
        for child in &node.children {
            render_node(child, depth + 1, out);
        }
        out.push_str("</ul>");
    }
    out.push_str("</details></li>");
}

pub fn render_html(tree: &DisplayTree) -> String {
    let mut html = String::from(
        r#"<!doctype html><html lang="zh-Hant"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>主論點樹</title><style>
:root{color-scheme:light}body{font:15px/1.65 system-ui,"Noto Sans TC",sans-serif;background:#f5f7fc;color:#1d2a3d;max-width:960px;margin:24px auto;padding:0 16px}h1{font-size:20px}p.intro{color:#65758e}ul{list-style:none;padding-left:18px;border-left:2px solid #dbe3f0}li{margin:9px 0}details{border:1px solid #dbe3f0;border-left:4px solid #4168c0;border-radius:9px;background:white;overflow:hidden}summary{cursor:pointer;padding:9px 11px;list-style:none}summary::-webkit-details-marker{display:none}summary:hover{background:#edf3ff}strong{font-weight:680}.tag{display:inline-block;background:#e8eef9;color:#375584;font-size:12px;border-radius:5px;padding:2px 6px;margin-right:8px}.round{float:right;color:#65758e;font-size:12px}.quote{font-size:13px;color:#52627b;margin:0;padding:0 12px 10px}details>ul{margin:0 12px 12px 16px}body>ul{padding-left:0;border:0}li li details{border-left-color:#29916b}li li li details{border-left-color:#b57d36}@media(max-width:600px){body{margin:12px auto;padding:0 10px}ul{padding-left:10px}.round{float:none;display:inline-block;margin-left:6px}}
</style></head><body><h1>"#,
    );
    html.push_str(&escape_html(&tree.title));
    html.push_str("</h1><p class=intro>主論點 → 子論點；點擊可逐層展開／收合。所有引用均來自原文，結果尚未人工複核。</p><ul>");
    render_node(&tree.root, 0, &mut html);
    html.push_str("</ul></body></html>");
    html
}

/// Typed, masked model payload. Never forward identity metadata or source-message bookkeeping.
pub fn model_input(input: &Transcript) -> Result<Value, TreeError> {
    let masked = sanitized_transcript(input)?;
    let rounds: Vec<Value> = masked.rounds.iter().enumerate().map(|(i, r)| {
        serde_json::json!({"round":i+1, "ai_question":r.ai_question, "user_answer":r.user_answer,
                          "question_type":r.extra.get("question_type").and_then(Value::as_str)})
    }).collect();
    Ok(serde_json::json!({"topic_id":masked.topic_id,"motion":masked.motion,"rounds":rounds}))
}

#[derive(Debug)]
pub struct RawCompletion {
    pub content: String,
    pub model: String,
    pub usage: Option<Value>,
    pub request_id: Option<String>,
}

pub struct MainTreeClient {
    http: reqwest::Client,
    base_url: String,
    key: String,
    model: String,
    reasoning_effort: Option<String>,
}

impl MainTreeClient {
    pub fn from_env() -> Result<Self, TreeError> {
        let model = std::env::var("ANNOTATION_MODEL")
            .map_err(|_| TreeError::Invalid("請設定 ANNOTATION_MODEL".into()))?;
        if model.trim().is_empty() {
            return Err(TreeError::Invalid("ANNOTATION_MODEL 不可為空".into()));
        }
        let url_str = std::env::var("ANNOTATION_BASE_URL")
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let url = reqwest::Url::parse(&url_str)
            .map_err(|_| TreeError::Invalid("API base URL 無效".into()))?;
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")))
        {
            return Err(TreeError::Invalid(
                "只有 HTTPS 或 localhost HTTP 可用".into(),
            ));
        }
        let key = load_openai_keys()
            .map_err(|_| TreeError::Invalid("無法讀取 API 金鑰".into()))?
            .into_iter()
            .next()
            .ok_or_else(|| TreeError::Invalid("沒有可用的 API 金鑰".into()))?;
        let secs = std::env::var("ANNOTATION_TIMEOUT_SECS")
            .ok()
            .and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(300)
            .clamp(15, 900);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(secs))
            .build()
            .map_err(|_| TreeError::Api("HTTP client 建立失敗".into()))?;
        let effort = std::env::var("ANNOTATION_REASONING_EFFORT")
            .or_else(|_| std::env::var("OPENAI_REASONING_EFFORT"))
            .ok();
        let reasoning_effort = parse_reasoning_effort(effort.as_deref())?;
        Ok(Self {
            http,
            base_url: url_str.trim_end_matches('/').into(),
            key,
            model,
            reasoning_effort,
        })
    }

    pub fn request_body(&self, input: &Transcript) -> Result<Value, TreeError> {
        let masked = sanitized_transcript(input)?;
        let schema: Value = serde_json::from_str(OUTPUT_SCHEMA).map_err(TreeError::Json)?;
        let input = model_input(&masked)?;
        let mut body = serde_json::json!({
            "model": self.model, "stream": false, "store": false,
            "instructions": SYSTEM_PROMPT,
            "input": [{"role":"user", "content":serde_json::to_string(&input).map_err(TreeError::Json)?}],
            "text": {"format": {"type":"json_schema", "name":"main_argument_tree", "strict":true, "schema":schema}}
        });
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning"] = serde_json::json!({"effort":effort});
        }
        Ok(body)
    }

    pub async fn complete(&self, input: &Transcript) -> Result<RawCompletion, TreeError> {
        let body = self.request_body(input)?;
        let response = self
            .http
            .post(format!("{}/responses", self.base_url))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .map_err(|_| TreeError::Api("API 請求失敗或逾時".into()))?;
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|x| x.to_str().ok())
            .map(ToOwned::to_owned);
        if !response.status().is_success() {
            return Err(TreeError::Api(format!(
                "HTTP {}（請檢查 Responses endpoint、模型、reasoning effort 與 strict json_schema 支援）",
                response.status()
            )));
        }
        let answer: Value = response
            .json()
            .await
            .map_err(|_| TreeError::Api("API 回應並非 JSON".into()))?;
        parse_response(&answer, &self.model, request_id)
    }
}

fn parse_reasoning_effort(value: Option<&str>) -> Result<Option<String>, TreeError> {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(v @ ("none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max")) => Ok(Some(v.into())),
        Some(_) => Err(TreeError::Invalid("reasoning effort 無效；使用 none/minimal/low/medium/high/xhigh/max（支援值依模型而定）".into())),
    }
}

/// Adapt the Responses wire envelope to the original RawCompletion contract.
/// Reasoning items are never mixed into the JSON payload or persisted.
pub fn parse_response(
    answer: &Value,
    fallback_model: &str,
    request_id: Option<String>,
) -> Result<RawCompletion, TreeError> {
    if answer["status"].as_str() != Some("completed") || !answer["error"].is_null() {
        return Err(TreeError::Api(
            "Responses 未完成或失敗；不接受部分輸出".into(),
        ));
    }
    let output = answer["output"]
        .as_array()
        .ok_or_else(|| TreeError::Api("Responses 缺少 output".into()))?;
    let mut content = String::new();
    for item in output {
        match item["type"].as_str() {
            Some("reasoning") => continue,
            Some("message") if item["role"] == "assistant" && item["status"] == "completed" => {
                let parts = item["content"]
                    .as_array()
                    .ok_or_else(|| TreeError::Api("Responses 訊息缺少 content".into()))?;
                for part in parts {
                    match part["type"].as_str() {
                        Some("refusal") => return Err(TreeError::Api("模型拒答".into())),
                        Some("output_text") => {
                            let text = part["text"]
                                .as_str()
                                .ok_or_else(|| TreeError::Api("Responses 缺少文字".into()))?;
                            if content.len().saturating_add(text.len()) > MAX_RAW_BYTES {
                                return Err(TreeError::Invalid("模型輸出超出限制".into()));
                            }
                            content.push_str(text);
                        }
                        _ => return Err(TreeError::Api("Responses 含非預期的內容類型".into())),
                    }
                }
            }
            _ => {
                return Err(TreeError::Api(
                    "Responses 含未完成訊息或非預期的輸出類型".into(),
                ));
            }
        }
    }
    if content.trim().is_empty() {
        return Err(TreeError::Invalid("模型輸出空白".into()));
    }
    let usage = answer.get("usage").filter(|v| v.is_object()).map(|v| {
        let mut usage = v.clone();
        for (new, old) in [
            ("input_tokens", "prompt_tokens"),
            ("output_tokens", "completion_tokens"),
            ("input_tokens_details", "prompt_tokens_details"),
            ("output_tokens_details", "completion_tokens_details"),
        ] {
            if let Some(value) = v.get(new) {
                usage[old] = value.clone();
            }
        }
        usage
    });
    Ok(RawCompletion {
        content,
        model: answer["model"].as_str().unwrap_or(fallback_model).into(),
        usage,
        request_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript() -> Transcript {
        serde_json::from_str(include_str!(
            "../eval/main_argument_tree/fixtures/synthetic_transcript.json"
        ))
        .unwrap()
    }
    fn outline() -> Outline {
        serde_json::from_str(include_str!(
            "../eval/main_argument_tree/fixtures/synthetic_outline.json"
        ))
        .unwrap()
    }
    #[test]
    fn main_argument_tree_valid_synthetic() {
        let tree = validate_and_build(&transcript(), &outline()).unwrap();
        assert_eq!(tree.root.node.id, "TEST-C0");
        assert_eq!(tree.node_count, 5);
        assert_eq!(tree.root.children.len(), 2);
        assert_eq!(
            tree.root.children[1].children[0].node.kind,
            NodeKind::IndependentJudgment
        );
        assert!(render_html(&tree).contains("尚無完整檢查遺漏方法"));
        assert!(render_html(&tree).contains("個人判斷／未定"));
    }
    #[test]
    fn main_argument_tree_reject_missing_parent() {
        let mut data = outline();
        data.nodes[1].parent = Some("TEST-FAKE".into());
        assert!(validate_and_build(&transcript(), &data).is_err());
    }
    #[test]
    fn main_argument_tree_reject_bad_quote() {
        let mut data = outline();
        data.nodes[1].evidence.push_str("不存在");
        assert!(validate_and_build(&transcript(), &data).is_err());
    }
    #[test]
    fn main_argument_tree_reject_cycle() {
        let mut data = outline();
        data.nodes[1].parent = Some("TEST-J1".into());
        data.nodes[3].parent = Some("TEST-P1".into());
        assert!(validate_and_build(&transcript(), &data).is_err());
    }
    #[test]
    fn main_argument_tree_reject_two_roots() {
        let mut data = outline();
        data.nodes[1].parent = None;
        data.nodes[1].relation = None;
        assert!(validate_and_build(&transcript(), &data).is_err());
    }
    #[test]
    fn main_argument_tree_reject_wrong_ai_origin() {
        let mut data = outline();
        data.nodes[1].origin = Origin::AssistantQuestionScenario;
        assert!(validate_and_build(&transcript(), &data).is_err());
    }
    #[test]
    fn main_argument_tree_does_not_require_final_closure() {
        let tree = validate_and_build(&transcript(), &outline()).unwrap();
        assert_eq!(tree.root.children[1].children[0].node.source_round, 3);
        assert_eq!(
            tree.root.children[1].children[0].node.kind,
            NodeKind::IndependentJudgment
        );
    }

    #[test]
    fn main_argument_tree_sanitizes_sensitive_input() {
        let mut input = transcript();
        input.rounds[0]
            .user_answer
            .push_str("，聯絡 student@example.com");
        let masked = sanitized_transcript(&input).unwrap();
        assert!(!masked.rounds[0].user_answer.contains("student@example.com"));
        assert!(masked.rounds[0].user_answer.contains("[EMAIL]"));
        assert_eq!(masked.rounds.len(), input.rounds.len());
    }

    #[test]
    fn main_argument_tree_masks_text_before_json_escaping() {
        let mut input = transcript();
        let text = "第一行\nuser@example.com\r\n12345678A\t0912345678\n引號\"與路徑 C:\\notes\\file.txt 😀";
        let expected =
            "第一行\n[EMAIL]\r\n[STUDENT_ID]\t[PHONE]\n引號\"與路徑 C:\\notes\\file.txt 😀";
        input.motion = text.into();
        input.rounds[0].ai_question = text.into();
        input.rounds[0].user_answer = text.into();
        // Identifiers are structural, even when they resemble a masking pattern.
        input.topic_id = "0912345678".into();
        for (i, round) in input.rounds.iter_mut().enumerate() {
            round.id = Some(format!("0912345678-R{:02}", i + 1));
        }
        let masked = sanitized_transcript(&input).unwrap();
        assert_eq!(masked.motion, expected);
        assert_eq!(masked.rounds[0].ai_question, expected);
        assert_eq!(masked.rounds[0].user_answer, expected);
        assert_eq!(masked.topic_id, input.topic_id);
        assert_eq!(masked.rounds[0].id, input.rounds[0].id);
        assert_eq!(input.rounds[0].user_answer, text);
        let encoded = serde_json::to_string(&masked).unwrap();
        let decoded: Transcript = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.rounds[0].user_answer, expected);
    }

    #[test]
    fn main_argument_tree_masks_nested_metadata_without_changing_types() {
        let mut input = transcript();
        input.extra.insert(
            "metadata".into(),
            serde_json::json!({"contact":"\nuser@example.com", "items":["\t12345678A", 7, true, null]}),
        );
        input.rounds[0].extra.insert(
            "note".into(),
            serde_json::json!([{"contact":"\n0912345678"}]),
        );
        let masked = sanitized_transcript(&input).unwrap();
        assert_eq!(masked.extra["metadata"]["contact"], "\n[EMAIL]");
        assert_eq!(
            masked.extra["metadata"]["items"],
            serde_json::json!(["\t[STUDENT_ID]", 7, true, null])
        );
        assert_eq!(masked.rounds[0].extra["note"][0]["contact"], "\n[PHONE]");
        assert_eq!(input.extra["metadata"]["contact"], "\nuser@example.com");
    }

    #[test]
    fn main_argument_tree_rejects_wrong_round_identifier() {
        let mut input = transcript();
        input.rounds[0].id = Some("TEST-R99".into());
        assert!(input.validate().is_err());
    }

    #[test]
    fn main_argument_tree_reject_uncertain_as_support() {
        let mut data = outline();
        let uncertain = data.nodes.iter_mut().find(|n| n.id == "TEST-J1").unwrap();
        uncertain.relation = Some(Relation::Support);
        assert!(validate_and_build(&transcript(), &data).is_err());
    }

    #[test]
    fn main_argument_tree_reject_precision_as_qualify() {
        let mut data = outline();
        let precise = data.nodes.iter_mut().find(|n| n.id == "TEST-PR1").unwrap();
        precise.relation = Some(Relation::Qualify);
        assert!(validate_and_build(&transcript(), &data).is_err());
    }

    #[test]
    fn main_argument_tree_reject_nested_same_round_judgment() {
        let mut data = outline();
        let mut duplicated = data
            .nodes
            .iter()
            .find(|n| n.id == "TEST-J1")
            .unwrap()
            .clone();
        duplicated.id = "TEST-J2".into();
        duplicated.parent = Some("TEST-J1".into());
        data.nodes.push(duplicated);
        assert!(validate_and_build(&transcript(), &data).is_err());
    }

    #[test]
    fn main_argument_tree_schema_contract() {
        let schema: Value = serde_json::from_str(OUTPUT_SCHEMA).unwrap();
        assert_eq!(schema["additionalProperties"], false);
        let fields = schema["properties"]["nodes"]["items"]["required"]
            .as_array()
            .unwrap();
        assert_eq!(fields.len(), 8);
        assert!(fields.iter().any(|v| v.as_str() == Some("parent")));
        assert!(fields.iter().any(|v| v.as_str() == Some("relation")));
        assert!(
            schema["properties"]["nodes"]["items"]["properties"]["kind"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some("independent_judgment"))
        );
    }
}

#[cfg(test)]
mod responses_tests {
    use super::*;
    use serde_json::json;

    fn input() -> Transcript {
        serde_json::from_str(include_str!(
            "../eval/main_argument_tree/fixtures/synthetic_transcript.json"
        ))
        .unwrap()
    }
    fn response() -> Value {
        json!({"status":"completed","model":"test-model","output":[
            {"type":"reasoning","summary":[]},
            {"type":"message","role":"assistant","status":"completed","content":[
                {"type":"output_text","text":"{\"ok\":"},
                {"type":"output_text","text":"true}"}]}],
            "usage":{"input_tokens":10,"output_tokens":20,"total_tokens":30,
                "input_tokens_details":{"cached_tokens":2},
                "output_tokens_details":{"reasoning_tokens":8}}})
    }
    #[test]
    fn main_argument_tree_responses_adapter_preserves_contract() {
        let result = parse_response(&response(), "fallback", Some("req-test".into())).unwrap();
        assert_eq!(result.content, "{\"ok\":true}");
        assert_eq!(result.model, "test-model");
        assert_eq!(result.request_id.as_deref(), Some("req-test"));
        let usage = result.usage.unwrap();
        assert_eq!(usage["prompt_tokens"], 10);
        assert_eq!(usage["completion_tokens_details"]["reasoning_tokens"], 8);
    }
    #[test]
    fn main_argument_tree_responses_rejects_partial_refusal_and_empty() {
        for status in ["incomplete", "failed", "cancelled", "in_progress", "queued"] {
            let mut value = response();
            value["status"] = json!(status);
            assert!(parse_response(&value, "model", None).is_err());
        }
        let mut value = response();
        value["output"][1]["content"][1] = json!({"type":"refusal","refusal":"private"});
        assert!(parse_response(&value, "model", None).is_err());
        value["output"] = json!([{"type":"reasoning","summary":[]}]);
        assert!(parse_response(&value, "model", None).is_err());
        value = response();
        value["output"][1]["status"] = json!("incomplete");
        assert!(parse_response(&value, "model", None).is_err());
    }
    #[test]
    fn main_argument_tree_responses_request_and_effort() {
        for effort in [
            None,
            Some("none"),
            Some("minimal"),
            Some("low"),
            Some("medium"),
            Some("high"),
            Some("xhigh"),
            Some("max"),
        ] {
            let c = MainTreeClient {
                http: reqwest::Client::new(),
                base_url: "http://localhost/v1".into(),
                key: "test".into(),
                model: "test".into(),
                reasoning_effort: parse_reasoning_effort(effort).unwrap(),
            };
            let body = c.request_body(&input()).unwrap();
            assert_eq!(body["store"], false);
            assert_eq!(
                body["text"]["format"]["schema"],
                serde_json::from_str::<Value>(OUTPUT_SCHEMA).unwrap()
            );
            assert!(body.get("messages").is_none());
            assert!(body.get("response_format").is_none());
            assert_eq!(body["reasoning"]["effort"].as_str(), effort);
        }
        assert!(parse_reasoning_effort(Some("hihg")).is_err());
        assert_eq!(parse_reasoning_effort(Some(" ")).unwrap(), None);
    }
    #[test]
    fn main_argument_tree_chat_pairs_preceding_question_only() {
        let raw = json!({"id":"conversation-test","topic_id":null,"title":"工具使用","description":"如何平衡效率與責任？","generating":false, "student_name":"PRIVATE", "messages":[
            {"role":"student","content":"我支持。"},
            {"role":"ai","content":"為什麼？","question_type":"reason"},
            {"role":"student","content":"可以省時。"},
            {"role":"ai","content":"討論結束。","question_type":"wrap_up"}]});
        let result = parse_transcript(&raw.to_string()).unwrap();
        assert_eq!(result.topic_id, "conversation-test");
        assert_eq!(result.motion, "工具使用\n\n如何平衡效率與責任？");
        assert_eq!(result.rounds.len(), 2);
        assert_eq!(result.rounds[0].ai_question, "");
        assert_eq!(result.rounds[1].ai_question, "為什麼？");
        assert_eq!(result.rounds[1].extra["question_type"], "reason");
        assert!(!serde_json::to_string(&result).unwrap().contains("PRIVATE"));
    }

    #[test]
    fn main_argument_tree_chat_merges_adjacent_student_messages() {
        let raw = json!({"conversation_id":"C", "title":"工具使用", "messages":[
            {"role":"ai", "content":"理由？", "question_type":"reason"},
            {"role":"student", "content":"可以省時。"},
            {"role":"student", "content":"但必須檢查。"},
            {"role":"user", "content":"我再補充。"},
            {"role":"ai", "content":"怎麼檢查？", "question_type":"clarify"},
            {"role":"student", "content":"查證來源。"},
            {"role":"ai", "content":"結束。"}]});
        let input = parse_transcript(&raw.to_string()).unwrap();
        assert_eq!(input.rounds.len(), 2);
        assert_eq!(
            input.rounds[0].user_answer,
            "可以省時。\n但必須檢查。\n我再補充。"
        );
        assert_eq!(input.rounds[0].ai_question, "理由？");
        assert_eq!(input.rounds[0].extra["question_type"], "reason");
        assert_eq!(
            input.rounds[0].extra["source_message_indices"],
            json!([2, 3, 4])
        );
        assert_eq!(input.rounds[1].round, Some(2));
        assert_eq!(input.rounds[1].ai_question, "怎麼檢查？");
        assert_eq!(input.rounds[1].extra["question_type"], "clarify");
        assert_eq!(input.rounds[1].extra["source_message_indices"], json!([6]));
        let client = MainTreeClient {
            http: reqwest::Client::new(),
            base_url: "http://localhost/v1".into(),
            key: "test".into(),
            model: "test".into(),
            reasoning_effort: None,
        };
        let body = client.request_body(&input).unwrap();
        let sent: Value =
            serde_json::from_str(body["input"][0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(
            sent["rounds"][0]["user_answer"],
            input.rounds[0].user_answer
        );
        assert!(sent["rounds"][0].get("source_message_indices").is_none());
    }

    #[test]
    fn main_argument_tree_chat_preserves_initial_fragments_and_ai_boundaries() {
        let raw = json!({"id":"C", "title":"T", "messages":[
            {"role":"user", "content":"同意。"},
            {"role":"student", "content":"同意。"},
            {"role":"assistant", "content":"為什麼？"},
            {"role":"ai", "content":"請舉例。", "question_type":"reason"},
            {"role":"student", "content":"因為\\n是兩個字元。"}]});
        let result = parse_transcript(&raw.to_string()).unwrap();
        assert_eq!(result.rounds.len(), 2);
        assert_eq!(result.rounds[0].ai_question, "");
        assert_eq!(result.rounds[0].user_answer, "同意。\n同意。");
        assert_eq!(
            result.rounds[0].extra["source_message_indices"],
            json!([1, 2])
        );
        assert_eq!(result.rounds[1].ai_question, "為什麼？\n請舉例。");
        assert_eq!(result.rounds[1].user_answer, "因為\\n是兩個字元。");
    }

    #[test]
    fn main_argument_tree_chat_rejects_ambiguous_or_invalid_input() {
        for messages in [
            json!([]),
            json!([{"role":"system","content":"x"}]),
            json!([{"role":"student","content":"a"},{"role":"student","content":" "}]),
        ] {
            assert!(
                parse_transcript(
                    &json!({"conversation_id":"C","title":"T","messages":messages}).to_string()
                )
                .is_err()
            );
        }
        let original = input();
        assert_eq!(
            parse_transcript(&serde_json::to_string(&original).unwrap())
                .unwrap()
                .rounds
                .len(),
            original.rounds.len()
        );
        let mut empty = original.clone();
        empty.rounds[0].ai_question.clear();
        assert!(empty.validate().is_ok());
        empty.rounds[1].ai_question.clear();
        assert!(empty.validate().is_err());
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use axum::{Json, Router, routing::post};

    #[tokio::test]
    async fn main_argument_tree_responses_http_to_original_tree() {
        let app = Router::new().route("/v1/responses", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["reasoning"]["effort"], "high");
            assert_eq!(body["text"]["format"]["type"], "json_schema");
            let input:Value = serde_json::from_str(body["input"][0]["content"].as_str().unwrap()).unwrap();
            assert!(input.get("student_name").is_none());
            assert_eq!(input["rounds"][0]["round"],1);
            Json(serde_json::json!({"status":"completed","model":"mock",
                "output":[{"type":"reasoning","summary":[]},
                {"type":"message","role":"assistant","status":"completed","content":[
                    {"type":"output_text","text":include_str!("../eval/main_argument_tree/fixtures/synthetic_outline.json")}
                ]}]}))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = MainTreeClient {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            base_url: format!("http://{addr}/v1"),
            key: "test".into(),
            model: "mock".into(),
            reasoning_effort: Some("high".into()),
        };
        let mut input: Transcript = serde_json::from_str(include_str!(
            "../eval/main_argument_tree/fixtures/synthetic_transcript.json"
        ))
        .unwrap();
        input
            .extra
            .insert("student_name".into(), Value::String("PRIVATE".into()));
        let completion = client.complete(&input).await.unwrap();
        let (_, tree) =
            parse_and_build(&sanitized_transcript(&input).unwrap(), &completion.content).unwrap();
        assert_eq!(tree.node_count, 5);
        assert!(render_html(&tree).contains("尚無完整檢查遺漏方法"));
        task.abort();
    }
}
