# F-08.2-AI 收斂判定評估

對應 [Issue #190：收斂判定：AI 提示與行為評估](https://github.com/2026software-3/socrates-chat/issues/190)。規格依據為 `docs/specs/technical-details.md` 的 S-03.3（對話回合上限、主動收尾）與 `docs/specs/decisions.md` 的 S-03.4（AI 行為評分準則與情境集）；本文記錄目前評估實作，不新增或取代產品規格。

> **狀態：尚未實際跑過評估，沒有通過率數字。** 目前只有離線測試通過（版本欄位與結果結構、情境檔可解析、格式檢查）；沒有任何一輪真實模型的結果可以引用，也沒有人工複核紀錄。第一輪真實評估跑完後，請把 Run ID、模型與結果補到下方「評估紀錄」。

## 要解決的問題

Given 固定情境達到收斂條件，When AI 回應，Then 適時收斂，而非無限追問。兩個方向的錯誤都要抓：

- **過晚收斂**：學生已在第 3 階段實質回應反例（或已到引導收尾回合），AI 仍繼續拋新的挑戰。
- **過早收斂**：學生逃避反例或尚未完成反思，AI 卻提前收尾。

提示詞本身在 `backend/src/ai.rs` 的 `build_system_prompt`，F-08.2 的規則於 `v1.15` 整合進去（第 3 階段收斂條件判定、禁止反例套娃、達引導收尾回合時由 AI 主動梳理總結）。本 Issue 補的是固定情境集與評估 runner，以及結果裡的版本記錄。

## 檔案與版本

| 項目 | 位置／目前版本 |
| --- | --- |
| 受測提示詞 | `backend/src/ai.rs` 的 `build_system_prompt` |
| 提示詞版本 | `rules_version("zh-TW")`，目前為 `v1.23/zh-TW`；runner 直接呼叫 `rules_version`，結果記錄的就是產品當下使用的版本，不是另外抄一份 |
| 評估 runner | `backend/examples/try_f08.rs` |
| 評估程式版本 | `EVALUATOR_VERSION`：`f08-eval-v1` |
| 評審提示與準則 | runner 的 `JUDGE_PROMPT`（只有繁體中文版） |
| 評審準則版本 | `JUDGE_VERSION`：`f08-rubric-v1/zh-TW` |
| 結果格式版本 | `SCHEMA_VERSION`：`1` |
| 評估語言 | `zh-TW`（`LANGUAGE`）；情境與評審準則目前只有繁體中文版 |
| 固定情境 | `backend/eval/f08_2_cases.json`（8 題，全為合成資料） |

修改受測提示詞時更新 `ai.rs` 的 `RULES_VERSION`（及 `rules_version`）；修改評審準則 `JUDGE_PROMPT` 時更新 `JUDGE_VERSION`；修改評估邏輯（評分、重試、通過條件）時更新 `EVALUATOR_VERSION`；結果欄位有不相容變更時更新 `SCHEMA_VERSION`。這四個版本各自獨立：只改其中一個，就只有那個版本號變。

## 執行方式

API key 放在專案根目錄的 `.apikeylist`（一行一把，見 `.apikeylist.example`），或用 `OPENAI_API_KEYS`；專案根目錄 `.env`（runner 讀取 `backend/../.env`，必須存在）另需 `OPENAI_MODEL`、`OPENAI_JUDGE_MODEL`，必要時加 `OPENAI_BASE_URL`。受測與評審共用同一組 key 與 base URL。

```bash
cd backend
cargo test --example try_f08   # 離線測試，不呼叫 API
cargo run --example try_f08    # 真實模型評估：受測 8 題（格式錯誤時每題最多嘗試 3 次）＋每題評審（輸出異常時最多嘗試 3 次），會使用 API 額度
```

可用環境變數調整：

| 變數 | 預設 | 說明 |
| --- | --- | --- |
| `EVAL_FIRST_TOKEN_SECS` | 30 | 首個 token 逾時（受測與評審共用） |
| `EVAL_TOTAL_SECS` | 90 | 完整回覆逾時（受測與評審共用） |
| `EVAL_DELAY_SECS` | 2 | 題與題之間暫停的秒數，用來避開限流 |
| `CASE` | 未設定 | 只跑指定題號，例如 `CASE=CONV_01` |

結果寫在 `backend/eval/results/f08_<run-id>/`（`<run-id>` 是執行時的 Unix 秒數）。該目錄被 `.gitignore` 排除，不會進版本控制；要保留的紀錄請整理進本文件的「評估紀錄」。

## 固定情境集

全部為合成的第 3 階段（挑戰）對話，每題帶 `expected_advance`、`expected_type`、`acceptable_types`、`learning_goal` 與 `annotation_reason`。

| 類別 | 題號 | 預期 | 設計重點 |
| --- | --- | --- | --- |
| 已達收斂條件 | `CONV_01`–`CONV_04` | `advance=true`、`wrap_up` | 學生已針對具體反例做出有理由的區分或調適（電車難題、校園手機管理、說謊與誠實、資源分配） |
| 尚未達收斂條件 | `NOT_CONV_01`–`NOT_CONV_03` | `advance=false`，且**不得**收尾（可接受 `perspective`／`counterexample`／`clarify`／`assumption`） | 學生逃避反例或沒有完成反思（志工服務、隱私與校園安全、死刑存廢） |
| 回合上限 | `TURN_LIMIT_01` | `advance=true`、`wrap_up` | 第 15 回合、僅在第 2 階段，學生條件未全達成，仍須收尾（安樂死合法化） |

## 評估流程

1. **程式檢查**（不花 API，`grade`）：回覆需有正文、`<<<META>>>` 與合法 JSON，且含 `question_type`（六種之一）、`advance`（布林）、非空的 `reason`。格式合格後比對 `advance` 是否等於 `expected_advance`、`question_type` 是否為 `expected_type` 或 `acceptable_types`。結果為 `PASS`／`MISMATCH`／`FORMAT_ERROR`。格式錯誤會自動重試，每題最多嘗試 3 次。
2. **評審模型**（`JUDGE_PROMPT`）：看得到題目、階段與回合、預期是否收斂、完整對話與待評的回覆，輸出兩個判定：
   - `advance_verdict`（`PASS`／`FAIL`）：AI 的 `advance` 判斷是否正確。
   - `convergence_verdict`（`PASS`／`FAIL`／`UNCERTAIN`）：正文的引導行動是否適時收斂（該收時不再拋新難題，不該收時不提議收工）。
   評審輸出不是合法 JSON 時最多嘗試 3 次，仍失敗記為 `ERROR`。
3. **通過條件**：評審的 `advance_verdict` 與 `convergence_verdict` **都是 `PASS`** 才算該題通過。程式檢查的 `offline_status` 只記錄、不影響通過與否。這個通過條件是 runner 的設計，不是團隊約定的驗收門檻。

## 結果格式

每題一個 `<case_id>.json`，另有整輪的 `summary.json`。兩者都帶同一組版本與設定欄位：

| 欄位 | 說明 |
| --- | --- |
| `schema_version` | 結果格式版本 |
| `evaluator_version` | 評估程式版本 |
| `rules_version` | 受測提示詞版本（含語言後綴） |
| `language` | 評估語言 |
| `wrap_up_turn` | 受測提示詞使用的引導收尾回合（15） |
| `model_requested` / `judge_model` | 受測與評審模型名稱 |
| `judge_version` / `judge_prompt_language` | 評審準則版本與語言 |
| `run_id` | 這一輪的識別碼 |

每題 JSON 另有 `case_id`、`raw_reply`（受測模型原始輸出）、`offline_status`、`parsed`（程式檢查的解析結果）與 `judgment`（評審判定，含 `*_reason`；評審失敗時為 `ERROR` 並附 `raw`）。

`summary.json` 另有 `total_cases`、`evaluated_cases`（有拿到受測回覆的題數；評審失敗的題目也含在內，判定記為 `ERROR`，不算通過）、`no_result_cases`（受測 API 全部失敗、沒有回覆的題數）、`passed`、`pass_rate_evaluated_cases`（沒有任何題目被評分時為 `null`）、`acceptance_note`，以及每題一列的 `cases`。

沒有拿到受測回覆的題目（API 逾時或被拒絕）不會產生 `<case_id>.json`，只反映在 `summary.json` 的 `no_result_cases`；它們不算通過，也不算失敗，需要補跑。

## 評估紀錄

尚無。第一輪真實評估後請在此新增：Run ID、`rules_version`、受測與評審模型、各題結果與人工複核。失敗樣本保留原始紀錄，不以重跑的成功覆蓋。

## 已知限制

以下是撰寫本文件時對照現行程式與規格發現的落差；**未在本 Issue 修改**（會改變評估結果的可比性，需要團隊決定並升版 `EVALUATOR_VERSION`／`JUDGE_VERSION`）：

- **runner 沒有套用「收尾回合指令接在最後一則學生訊息後」。** 依 S-03.3，正式系統在第 `wrap_up_turn` 回合起，會把 `closing_instruction` 接在送給模型的最後一則學生訊息後面（`reply.rs` 的 `generate`）。`try_f08.rs` 只用 `build_system_prompt` 組請求，沒有這一步，所以 `TURN_LIMIT_01`（第 15 回合）測到的條件與正式系統不同，可能比正式系統更容易失敗。
- **評審準則與 v1.23 的收尾行為不完全對齊。** S-03.3 現行規格是 AI 以一兩句結束語直接結束討論、不提問；`JUDGE_PROMPT` 的措辭仍是「提議收尾、整理總結或引導回顧結論」，部分情境註解也寫「AI 應提議收尾」。「引導回顧結論」若被模型寫成提問，在現行規格下其實不符，但評審準則不會判它失敗。
- **runner 直接呼叫 `collect`，不經過後端的回覆流程。** 「後端確認條件成立才結束，模型過早標記 `wrap_up` 不會結束」這條保護不在評估範圍內；本評估只看模型本身的 `advance` 與正文。
- **評審與受測可能是同一個模型**（共用設定，預設可相同），自評有偏差風險；建議第一輪人工抽查所有 `PASS` 與 `ERROR` 的題目。
- **只有 8 題、只有 `zh-TW`**；沒有英文與西班牙文情境，也沒有第 1、2 階段的收斂（晉級）情境，只有一題回合上限。
- **每題只跑一次**；模型輸出有隨機性，單次通過不代表穩定。
- 自動評分不代表 Issue 已通過團隊驗收。
