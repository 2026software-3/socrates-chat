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
        build_system_prompt, collect, rules_version,
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
/// 給學生看的回覆有幾句（以句末標點計，沒有標點的非空文字算一句）；只用來記錄回覆長度。
fn count_sentences(text: &str) -> usize {
    let ends = text
        .chars()
        .filter(|c| matches!(c, '。' | '！' | '？' | '!' | '?' | '.'))
        .count();
    if ends == 0 && !text.trim().is_empty() {
        1
    } else {
        ends
    }
}

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
        "student_response_chars": response.chars().count(),
        "student_response_sentences": count_sentences(response),
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

/// 評估語言：F-08.2 的固定情境與評審準則目前都只有繁體中文版。
const LANGUAGE: &str = "zh-TW";
/// 受測提示詞使用的引導收尾回合（與正式系統預設相同）。
const WRAP_UP_TURN: u32 = 15;
/// 修改評估邏輯（評分、重試、通過條件）時更新。
const EVALUATOR_VERSION: &str = "f08-eval-v2";
/// 修改 `JUDGE_PROMPT` 評審準則時更新；後綴是評審準則的語言。
const JUDGE_VERSION: &str = "f08-rubric-v2/zh-TW";
const JUDGE_PROMPT_LANGUAGE: &str = "zh-TW";
/// 結果欄位有不相容變更時更新。
const SCHEMA_VERSION: u32 = 1;

/// 每筆結果與摘要都帶的版本與設定：之後才能知道這份結果是哪一版提示詞、哪一版評估程式與準則跑出來的。
fn run_metadata(model: &str, judge_model: &str) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "evaluator_version": EVALUATOR_VERSION,
        "rules_version": rules_version(LANGUAGE),
        "language": LANGUAGE,
        "wrap_up_turn": WRAP_UP_TURN,
        "model_requested": model,
        "judge_model": judge_model,
        "judge_version": JUDGE_VERSION,
        "judge_prompt_language": JUDGE_PROMPT_LANGUAGE,
    })
}

/// 單一情境的結果檔內容：版本與設定加上這一題的回覆與評分。
fn case_record(
    metadata: &Value,
    run_id: u64,
    case_id: &str,
    raw_reply: &str,
    offline_status: &str,
    parsed: &Value,
    judgment: &Value,
) -> Value {
    let mut record = metadata.clone();
    let fields = json!({
        "run_id": run_id,
        "case_id": case_id,
        "raw_reply": raw_reply,
        "offline_status": offline_status,
        "parsed": parsed,
        "judgment": judgment,
    });
    if let (Some(record), Some(fields)) = (record.as_object_mut(), fields.as_object()) {
        record.extend(fields.clone());
    }
    record
}

/// 每題一列的某個數值欄位平均；沒有任何列帶這個欄位時為 `null`。
fn average(rows: &[Value], field: &str) -> Option<f64> {
    let values: Vec<f64> = rows.iter().filter_map(|r| r[field].as_f64()).collect();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// 整輪的摘要。`rows` 只含有拿到受測回覆、評分過的題目；沒有回覆（API 失敗）的題目算在 `no_result_cases`。
/// 通過條件是評審的 advance 與 convergence 兩項都是 PASS；這不是團隊約定的驗收門檻。
fn summary(metadata: &Value, run_id: u64, total_cases: usize, rows: &[Value]) -> Value {
    let passed = rows
        .iter()
        .filter(|r| r["advance_verdict"] == "PASS" && r["convergence_verdict"] == "PASS")
        .count();
    let evaluated = rows.len();
    let mut out = metadata.clone();
    let fields = json!({
        "run_id": run_id,
        "total_cases": total_cases,
        "evaluated_cases": evaluated,
        "no_result_cases": total_cases - evaluated,
        "passed": passed,
        "pass_rate_evaluated_cases": if evaluated > 0 {
            Some(passed as f64 / evaluated as f64)
        } else {
            None
        },
        "avg_reply_chars": average(rows, "reply_chars"),
        "avg_reply_sentences": average(rows, "reply_sentences"),
        "acceptance_note": "評審結果不是團隊約定的驗收門檻；仍需人工抽查與團隊審閱。",
        "cases": rows,
    });
    if let (Some(out), Some(fields)) = (out.as_object_mut(), fields.as_object()) {
        out.extend(fields.clone());
    }
    out
}

/// 評審模型的 Prompt：評估受測模型是否「適時收斂」還是「無限追問」
const JUDGE_PROMPT: &str = r#"你是哲學討論對話的客觀評審。依據【S-03.3 收斂條件】評估 AI 回應是否適時收斂。

【情境資訊】
題目：{title}
題目說明：{desc}
目前階段：第 {stage} 階段，第 {turn} 回合
預期是否收斂：{expected_advance}（true 代表學生已對至少兩個角度不同的反例都實質回應、或達到回合上限，AI 應直接說結束語並結束討論；false 代表學生逃避反例、未完成反思，或只實質回應了一個反例，AI 應繼續：沒回應就追問，只回應一個就提出第二個角度不同的反例）

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
   - PASS：AI 的 advance 判斷正確！例如學生逃避反例、未完成思考，或只實質回應了一個反例時，AI 正確給予 false（判定 PASS）；當學生已對兩個角度不同的反例都實質回應，或達到 15 回合時，AI 正確給予 true（判定 PASS）。
   - FAIL：AI 的 advance 判斷錯誤！例如學生逃避反例、或只回應了一個反例，AI 卻誤給 true（過早收斂）；或學生已對兩個不同角度的反例充分回應，AI 依然給 false（無限追問）。
   ※ 特別注意：advance_verdict 是在評分「AI 是否判斷得當」。若學生逃避反例而 AI 正確判斷 advance=false，此時 advance_verdict 必須為 PASS！

2. convergence_action（評估「AI 在正文中的引導行動」）：
   - PASS：
     - 若達到收斂條件（expected true）：AI 直接說結束語並結束討論，回顧學生的立場、理由與面對不同反例時的轉折，不提問、不請學生確認或補充，也沒有再引入第三個反例。
     - 若未達收斂條件（expected false）：AI 指出學生逃避之處、繼續追問，或在學生已回應第一個反例時，具體回應他的判斷後提出第二個角度不同（不是同一情境換個說法）的反例或對立觀點；沒有說結束語，也沒有放棄引導。
   - FAIL：
     - 達到收斂條件時，AI 卻完全無視收斂時機，繼續拋出新的挑戰，或結束語仍向學生提問。
     - 未達收斂條件時，AI 卻直接說結束語、提議總結收工，或對已回應第一個反例的學生只換個說法重問同一個情境。
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
    // 與正式環境一致：有設 OPENAI_REASONING_EFFORT 才送（受測與評審共用）
    let effort = env::var("OPENAI_REASONING_EFFORT")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let provider =
        OpenAiProvider::new(&base_url, keys.clone(), &model).with_reasoning_effort(effort.clone());
    let judge_provider =
        OpenAiProvider::new(&base_url, keys.clone(), &judge_model).with_reasoning_effort(effort);

    println!(
        "Loaded {} test cases. Target model: {model}, Judge model: {judge_model}",
        cases.len()
    );
    println!(
        "Rules: {}; evaluator: {EVALUATOR_VERSION}; rubric: {JUDGE_VERSION}; schema: {SCHEMA_VERSION}",
        rules_version(LANGUAGE)
    );
    let metadata = run_metadata(&model, &judge_model);

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
    let mut rows: Vec<Value> = Vec::new();

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
                wrap_up_turn: WRAP_UP_TURN,
                language: LANGUAGE,
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

        let result_json = case_record(
            &metadata,
            run_id,
            &case.id,
            &raw_reply,
            &offline_status,
            &parsed,
            &judgment,
        );
        rows.push(json!({
            "case_id": case.id,
            "offline_status": offline_status,
            "advance_verdict": judgment["advance_verdict"],
            "convergence_verdict": judgment["convergence_verdict"],
            "reply_chars": parsed["student_response_chars"],
            "reply_sentences": parsed["student_response_sentences"],
            "file": format!("{}.json", case.id),
        }));

        save_json(&out_dir.join(format!("{}.json", case.id)), &result_json)?;
    }
    save_json(
        &out_dir.join("summary.json"),
        &summary(&metadata, run_id, cases.len(), &rows),
    )?;

    println!("\n==========================================");
    println!(
        "Evaluation complete: Passed {passed}/{total} ({:.1}%)",
        if total > 0 {
            passed as f64 / total as f64 * 100.0
        } else {
            0.0
        }
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
        assert_eq!(cases.len(), 12);
    }

    #[test]
    fn stage_three_cases_separate_one_counterexample_from_two() {
        // S-03.3：第 3 階段要至少兩個角度不同的反例都得到實質回應才收斂。
        // ONE_xx 只回應了第一個反例（不得收斂、不得收尾）；CONV_xx 是同一段對話再多一輪第二個反例並得到回應。
        let raw = include_str!("../eval/f08_2_cases.json");
        let cases: Vec<Case> = serde_json::from_str(raw).expect("valid test cases JSON");
        let user_turns = |c: &Case| c.conversation.iter().filter(|t| t.role == "user").count();
        let ones: Vec<&Case> = cases.iter().filter(|c| c.id.starts_with("ONE_")).collect();
        let convs: Vec<&Case> = cases.iter().filter(|c| c.id.starts_with("CONV_")).collect();
        assert_eq!(ones.len(), 4);
        assert_eq!(convs.len(), 4);
        for c in &ones {
            assert_eq!(c.stage, 3, "{}", c.id);
            assert!(!c.expected_advance, "{} 只回應一個反例不得收斂", c.id);
            assert!(
                c.expected_type != "wrap_up" && !c.acceptable_types.iter().any(|t| t == "wrap_up"),
                "{} 不得接受 wrap_up",
                c.id
            );
            assert_eq!(c.conversation.last().map(|t| t.role.as_str()), Some("user"));
            assert_eq!(user_turns(c), 3, "{} 應停在學生回應第一個反例", c.id);
        }
        for c in &convs {
            assert_eq!(c.stage, 3, "{}", c.id);
            assert!(c.expected_advance, "{} 兩個反例都回應後應收斂", c.id);
            assert_eq!(c.expected_type, "wrap_up", "{}", c.id);
            assert_eq!(c.conversation.last().map(|t| t.role.as_str()), Some("user"));
            assert_eq!(user_turns(c), 4, "{} 應多一輪第二個反例的回應", c.id);
        }
        // 成對：CONV_0n 的前 6 則訊息就是 ONE_0n（確保差別只在第二個反例）
        for (one, conv) in ones.iter().zip(convs.iter()) {
            assert_eq!(one.title, conv.title);
            for (a, b) in one.conversation.iter().zip(conv.conversation.iter()) {
                assert_eq!(
                    a.content, b.content,
                    "{} 與 {} 的前文應相同",
                    one.id, conv.id
                );
            }
        }
    }

    #[test]
    fn grade_records_how_long_the_student_facing_reply_is() {
        // v1.24 要讓回覆更像對話：每題記錄回覆字數與句數，之後才能比較改前改後（不影響通過與否）
        let raw = "你把「不讓醫療變成人情交易」放在親人之前，這個理由很清楚。不過換個角度試試：如果依病情輕重決定，年長但病況較輕的人永遠排在後面，你覺得公平嗎？\n<<<META>>>\n{\"advance\": false, \"question_type\": \"counterexample\", \"reason\": \"只回應一個反例\"}";
        let (_, val) = grade(raw, false, "counterexample", &[]);
        assert_eq!(val["student_response_sentences"], 2);
        assert!(val["student_response_chars"].as_u64().unwrap() > 40);
        assert_eq!(count_sentences("一句。兩句！三句？"), 3);
        assert_eq!(count_sentences("沒有句號"), 1);
        assert_eq!(count_sentences(""), 0);
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

    fn metadata() -> Value {
        run_metadata("target-model", "judge-model")
    }

    #[test]
    fn run_metadata_records_the_rules_evaluator_and_rubric_versions() {
        let m = metadata();
        assert_eq!(m["schema_version"], SCHEMA_VERSION);
        assert_eq!(m["language"], "zh-TW");
        // 受測提示詞版本必須是產品實際使用的那個，而不是 runner 自己抄一份
        assert_eq!(
            m["rules_version"],
            socrates_chat_backend::ai::rules_version("zh-TW")
        );
        assert!(m["rules_version"].as_str().unwrap().ends_with("/zh-TW"));
        assert!(
            m["evaluator_version"]
                .as_str()
                .unwrap()
                .starts_with("f08-eval-")
        );
        let judge = m["judge_version"].as_str().unwrap();
        assert!(judge.starts_with("f08-rubric-") && judge.ends_with("/zh-TW"));
        assert_eq!(m["judge_prompt_language"], "zh-TW");
        assert_eq!(m["wrap_up_turn"], 15);
        assert_eq!(m["model_requested"], "target-model");
        assert_eq!(m["judge_model"], "judge-model");
    }

    #[test]
    fn a_case_record_keeps_its_result_and_carries_the_run_metadata() {
        let parsed = json!({"advance": true, "question_type": "wrap_up"});
        let judgment = json!({"advance_verdict": "PASS", "convergence_verdict": "PASS"});
        let record = case_record(
            &metadata(),
            1234,
            "CONV_01",
            "原始回覆",
            "PASS",
            &parsed,
            &judgment,
        );
        assert_eq!(record["case_id"], "CONV_01");
        assert_eq!(record["run_id"], 1234);
        assert_eq!(record["raw_reply"], "原始回覆");
        assert_eq!(record["offline_status"], "PASS");
        assert_eq!(record["parsed"], parsed);
        assert_eq!(record["judgment"], judgment);
        for key in [
            "schema_version",
            "evaluator_version",
            "rules_version",
            "language",
            "judge_version",
            "model_requested",
            "judge_model",
        ] {
            assert_eq!(record[key], metadata()[key], "{key}");
        }
    }

    fn row(id: &str, advance: &str, convergence: &str) -> Value {
        json!({"case_id": id, "advance_verdict": advance, "convergence_verdict": convergence})
    }

    #[test]
    fn the_summary_records_versions_and_counts_only_double_passes() {
        let rows = vec![
            row("A", "PASS", "PASS"),
            row("B", "PASS", "FAIL"),
            row("C", "ERROR", "ERROR"),
            row("D", "PASS", "UNCERTAIN"),
        ];
        let s = summary(&metadata(), 99, 5, &rows);
        assert_eq!(s["run_id"], 99);
        assert_eq!(s["total_cases"], 5);
        assert_eq!(s["evaluated_cases"], 4);
        assert_eq!(s["no_result_cases"], 1);
        assert_eq!(s["passed"], 1);
        assert_eq!(s["pass_rate_evaluated_cases"], 0.25);
        assert_eq!(s["rules_version"], metadata()["rules_version"]);
        assert_eq!(s["evaluator_version"], metadata()["evaluator_version"]);
        assert_eq!(s["judge_version"], metadata()["judge_version"]);
        assert_eq!(s["schema_version"], SCHEMA_VERSION);
        assert_eq!(s["cases"].as_array().unwrap().len(), 4);
        assert!(s["acceptance_note"].as_str().unwrap().contains("人工"));
    }

    #[test]
    fn an_empty_run_has_no_pass_rate_instead_of_nan() {
        let s = summary(&metadata(), 1, 0, &[]);
        assert_eq!(s["evaluated_cases"], 0);
        assert!(s["pass_rate_evaluated_cases"].is_null());
    }
}
