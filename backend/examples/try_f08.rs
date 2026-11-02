//! Run from backend/: cargo run --example try_f08
//! Input: backend/eval/f08_2_cases.json (synthetic scenarios only).
//! Offline scorer tests: cargo test --example try_f08
//! Requires OPENAI_MODEL and OPENAI_JUDGE_MODEL; each valid response triggers a judge call.
//! Output: backend/eval/results/f08_<unique-run-id>/

use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use serde_json::{Value, json};
use socrates_chat_backend::{
    ai::{
        AiRequest, ChatRole, ChatTurn, META_DELIM, OpenAiProvider, PromptContext,
        build_system_prompt, collect,
    },
    config::load_openai_keys,
    key_pool::KeyPool,
};

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Case {
    id: String,
    stage: i16,
    turn: i32,
    title: String,
    description: String,
    conversation: Vec<InputTurn>,
    expected_advance: bool,
    expected_type: String,
    acceptable_types: Vec<String>,
    learning_goal: String,
    annotation_reason: String,
}

#[derive(Deserialize, Debug)]
struct InputTurn {
    role: String,
    content: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    dotenvy::from_path(root.join("../.env"))?;
    run()
}

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    let value = env::var(name)?;
    if value.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is empty")).into());
    }
    Ok(value)
}

fn save_json(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

const QUESTION_TYPES: [&str; 6] = [
    "clarify",
    "reason",
    "assumption",
    "counterexample",
    "perspective",
    "wrap_up",
];

/// 離線結構化檢查（格式、欄位、advance 旗標與 question_type）
fn grade(
    raw: &str,
    expected_advance: bool,
    expected_type: &str,
    acceptable_types: &[String],
) -> (String, Value) {
    let (response, meta_raw) = match raw.split_once(META_DELIM) {
        Some((response, meta)) => (response.trim(), Some(meta.trim())),
        None => (raw.trim(), None),
    };
    let mut errors: Vec<&str> = Vec::new();
    if response.is_empty() {
        errors.push("empty student-facing response");
    }
    let meta: Option<Value> = match meta_raw {
        None => {
            errors.push("missing META delimiter");
            None
        }
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(v) => Some(v),
            Err(_) => {
                errors.push("META is not valid JSON");
                None
            }
        },
    };

    let advance = meta
        .as_ref()
        .and_then(|m| m.get("advance"))
        .and_then(|a| a.as_bool());
    let question_type = meta
        .as_ref()
        .and_then(|m| m.get("question_type"))
        .and_then(|q| q.as_str());
    let reason = meta
        .as_ref()
        .and_then(|m| m.get("reason"))
        .and_then(|r| r.as_str());

    if advance.is_none() {
        errors.push("missing advance boolean");
    }
    match question_type {
        None => errors.push("missing question_type"),
        Some(qt) if !QUESTION_TYPES.contains(&qt) => errors.push("unknown question_type"),
        _ => {}
    }
    if reason.is_none_or(|r| r.trim().is_empty()) {
        errors.push("missing or empty reason");
    }

    let format_valid = errors.is_empty();
    let advance_match = advance == Some(expected_advance);
    let type_match = question_type
        .is_some_and(|qt| qt == expected_type || acceptable_types.iter().any(|acc| acc == qt));

    let grade_status = if !format_valid {
        "FORMAT_ERROR"
    } else if advance_match && type_match {
        "PASS"
    } else {
        "MISMATCH"
    };

    let parsed = json!({
        "format_valid": format_valid,
        "format_errors": errors,
        "student_response": response,
        "advance": advance,
        "advance_expected": expected_advance,
        "advance_match": advance_match,
        "question_type": question_type,
        "type_expected": expected_type,
        "type_match": type_match,
        "reason": reason,
    });

    (grade_status.to_string(), parsed)
}

/// 評審模型的 Prompt：評估受測模型是否「適時收斂」還是「無限追問」
const JUDGE_PROMPT: &str = r#"你是哲學討論對話的客觀評審。依據【S-03.3 收斂條件】評估 AI 回應是否適時收斂。

【情境資訊】
題目：{title}
題目說明：{desc}
目前階段：第 {stage} 階段，第 {turn} 回合
預期是否收斂：{expected_advance}（true 代表學生已妥善回應反例或達到回合上限，應提議收尾；false 代表學生逃避反例或未完成反思，應繼續追問）

【完整對話記錄】
{conversation}

【待評估的 AI 回覆】
給學生看的內容：
{student_response}

結構化 META：
advance: {advance}
question_type: {question_type}
reason: {reason}

【評分準則】
1. advance_appropriateness（評估「AI 的 advance 判斷是否正確」，合理為 PASS，不合理為 FAIL）：
   - PASS：AI 的 advance 判斷正確！例如學生逃避反例或未完成思考時，AI 正確給予 false（判定 PASS）；當學生實質回應反例或達到 15 回合時，AI 正確給予 true（判定 PASS）。
   - FAIL：AI 的 advance 判斷錯誤！例如學生逃避反例但 AI 卻誤給 true（過早收斂）；或學生已充分回應反例但 AI 依然給 false（無限追問）。
   ※ 特別注意：advance_verdict 是在評分「AI 是否判斷得當」。若學生逃避反例而 AI 正確判斷 advance=false，此時 advance_verdict 必須為 PASS！

2. convergence_action（評估「AI 在正文中的引導行動」）：
   - PASS：
     - 若達到收斂條件（expected true）：AI 提議收尾、整理總結或引導回顧結論，沒有再引入全新的反例難題。
     - 若未達收斂條件（expected false）：AI 指出學生逃避之處或繼續追問面對反例，沒有放棄引導。
   - FAIL：
     - 達到收斂條件時，AI 卻完全無視收斂時機，繼續拋出新的無關挑戰。
     - 未達收斂條件時，AI 卻直接提議總結收工。
   - UNCERTAIN：情境模糊難以斷定。

請只輸出一個合法 JSON 物件，不要有額外文字或 Markdown 區塊：
{
  "advance_verdict": "PASS 或 FAIL",
  "advance_reason": "簡述判定依據",
  "convergence_verdict": "PASS 或 FAIL 或 UNCERTAIN",
  "convergence_reason": "簡述 AI 在對話行動上是否適時收斂"
}
"#;

fn parse_judgment(raw: &str) -> Result<Value, &'static str> {
    let trimmed = raw.trim();
    let json_text = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start <= end {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    let v: Value = serde_json::from_str(json_text).map_err(|_| "judge returned invalid JSON")?;
    if !v["advance_verdict"]
        .as_str()
        .is_some_and(|s| ["PASS", "FAIL"].contains(&s))
    {
        return Err("invalid advance_verdict");
    }
    if !v["convergence_verdict"]
        .as_str()
        .is_some_and(|s| ["PASS", "FAIL", "UNCERTAIN"].contains(&s))
    {
        return Err("invalid convergence_verdict");
    }
    Ok(v)
}

#[tokio::main]
async fn run() -> Result<(), Box<dyn Error>> {
    let model = required_env("OPENAI_MODEL")?;
    let judge_model = required_env("OPENAI_JUDGE_MODEL")?;
    let base_url =
        env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());

    let raw_cases = fs::read_to_string("eval/f08_2_cases.json")?;
    let cases: Vec<Case> = serde_json::from_str(&raw_cases)?;

    let keys = Arc::new(
        KeyPool::new(load_openai_keys()?)
            .ok_or("no AI API key found")?
            .with_default_cooldown(Duration::from_secs(3)),
    );
    let provider = OpenAiProvider::new(&base_url, keys.clone(), &model);
    let judge_provider = OpenAiProvider::new(&base_url, keys.clone(), &judge_model);

    println!(
        "Loaded {} test cases. Target model: {model}, Judge model: {judge_model}",
        cases.len()
    );

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let out_dir = Path::new("eval/results").join(format!("f08_{run_id}"));
    fs::create_dir_all(&out_dir)?;

    let timeout_secs = |name: &str, default: u64| {
        env::var(name)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default)
    };
    let timeouts = (
        Duration::from_secs(timeout_secs("EVAL_FIRST_TOKEN_SECS", 30)),
        Duration::from_secs(timeout_secs("EVAL_TOTAL_SECS", 90)),
    );
    let delay = Duration::from_secs(timeout_secs("EVAL_DELAY_SECS", 2));
    let case_filter = env::var("CASE").ok();
    let cases: Vec<Case> = cases
        .into_iter()
        .filter(|c| {
            if let Some(ref target) = case_filter {
                &c.id == target
            } else {
                true
            }
        })
        .collect();

    let mut passed = 0;
    let mut total = 0;

    for case in &cases {
        if total > 0 && !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        total += 1;
        println!(
            "\n[{}/{}] Evaluating {} ({}) - Expected advance: {}",
            total,
            cases.len(),
            case.id,
            case.title,
            case.expected_advance
        );

        let messages: Vec<ChatTurn> = case
            .conversation
            .iter()
            .map(|turn| ChatTurn {
                role: if turn.role == "user" {
                    ChatRole::User
                } else {
                    ChatRole::Assistant
                },
                content: turn.content.clone(),
            })
            .collect();

        let req = AiRequest {
            system: build_system_prompt(&PromptContext {
                title: &case.title,
                description: &case.description,
                stage: case.stage,
                turn: case.turn,
                wrap_up_turn: 15,
                language: "zh-TW",
            }),
            messages,
        };

        let mut raw_reply = String::new();
        let mut offline_status = String::new();
        let mut parsed = Value::Null;

        for attempt in 1..=3 {
            let result = collect(&provider, req.clone(), timeouts.0, timeouts.1).await;
            let Ok(reply) = result else {
                if attempt == 3 {
                    println!("  [API_ERROR] Model call failed (timeout or API rejected)");
                }
                continue;
            };

            let (status, p) = grade(
                &reply,
                case.expected_advance,
                &case.expected_type,
                &case.acceptable_types,
            );

            raw_reply = reply;
            offline_status = status;
            parsed = p;

            if offline_status != "FORMAT_ERROR" || attempt == 3 {
                break;
            }
            println!("  [RETRY] 格式缺少 META，自動重試第 {} 次...", attempt + 1);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if raw_reply.is_empty() {
            continue;
        }

        println!(
            "  Offline grade: {offline_status} (advance={:?}, type={:?})",
            parsed["advance"], parsed["question_type"]
        );

        // 呼叫評審
        let mut conv_text = String::new();
        for t in &case.conversation {
            conv_text.push_str(&format!("{}: {}\n", t.role, t.content));
        }

        let judge_req_text = JUDGE_PROMPT
            .replace("{title}", &case.title)
            .replace("{desc}", &case.description)
            .replace("{stage}", &case.stage.to_string())
            .replace("{turn}", &case.turn.to_string())
            .replace("{expected_advance}", &case.expected_advance.to_string())
            .replace("{conversation}", &conv_text)
            .replace(
                "{student_response}",
                parsed["student_response"].as_str().unwrap_or(""),
            )
            .replace("{advance}", &parsed["advance"].to_string())
            .replace(
                "{question_type}",
                parsed["question_type"].as_str().unwrap_or("none"),
            )
            .replace("{reason}", parsed["reason"].as_str().unwrap_or(""));

        let judge_req = AiRequest {
            system: "你是一位嚴謹的教育評估評審。".into(),
            messages: vec![ChatTurn {
                role: ChatRole::User,
                content: judge_req_text,
            }],
        };

        let mut judgment = json!({
            "advance_verdict": "ERROR",
            "convergence_verdict": "ERROR",
            "raw": ""
        });

        for judge_attempt in 1..=3 {
            let judge_result =
                collect(&judge_provider, judge_req.clone(), timeouts.0, timeouts.1).await;
            let judge_reply = judge_result.unwrap_or_default();
            match parse_judgment(&judge_reply) {
                Ok(v) => {
                    judgment = v;
                    break;
                }
                Err(err) => {
                    if judge_attempt < 3 {
                        println!(
                            "  [JUDGE_RETRY] 評審輸出異常 ({err})，自動重試第 {} 次...",
                            judge_attempt + 1
                        );
                        tokio::time::sleep(Duration::from_millis(800)).await;
                    } else {
                        judgment = json!({
                            "advance_verdict": "ERROR",
                            "convergence_verdict": "ERROR",
                            "raw": judge_reply
                        });
                    }
                }
            }
        }

        println!(
            "  Judge verdict: advance={}, convergence={}",
            judgment["advance_verdict"], judgment["convergence_verdict"]
        );

        if judgment["advance_verdict"] == "PASS" && judgment["convergence_verdict"] == "PASS" {
            passed += 1;
        }

        let result_json = json!({
            "case_id": case.id,
            "raw_reply": raw_reply,
            "parsed": parsed,
            "judgment": judgment,
        });

        save_json(&out_dir.join(format!("{}.json", case.id)), &result_json)?;
    }

    println!("\n==========================================");
    println!(
        "Evaluation complete: Passed {passed}/{total} ({:.1}%)",
        (passed as f64 / total as f64) * 100.0
    );
    println!("Detailed results saved in: {}", out_dir.display());
    println!("==========================================");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_f08_test_cases() {
        let raw = include_str!("../eval/f08_2_cases.json");
        let cases: Vec<Case> = serde_json::from_str(raw).expect("valid test cases JSON");
        assert_eq!(cases.len(), 8);
    }

    #[test]
    fn grade_passes_on_valid_wrap_up_and_advance() {
        let raw = "我們已經充分討論了反例。你能為今天的想法做個收尾嗎？\n<<<META>>>\n{\"advance\": true, \"question_type\": \"wrap_up\", \"reason\": \"學生已回應反例\"}";
        let (status, val) = grade(raw, true, "wrap_up", &["wrap_up".into()]);
        assert_eq!(status, "PASS");
        assert_eq!(val["format_valid"], true);
        assert_eq!(val["advance_match"], true);
    }

    #[test]
    fn grade_fails_on_missing_meta() {
        let raw = "這是一個沒有 META 的回覆";
        let (status, val) = grade(raw, true, "wrap_up", &[]);
        assert_eq!(status, "FORMAT_ERROR");
        assert_eq!(val["format_valid"], false);
    }
}
