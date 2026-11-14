# F-24.1-AI 單場雷達圖評分評估

對應 [Issue #521](https://github.com/2026software-3/socrates-chat/issues/521)。規則以 `docs/specs/decisions.md` 的 S-04.2、S-04.3、S-04.6 與 `docs/specs/S-04-personality-radar.md` 為準；本文記錄評估實作，不新增或取代產品規格。

> **狀態：尚未用真實模型執行。** 目前只有離線檢查與測試替身的契約測試通過；評分品質與穩定性（同一份對話重複評分，每個維度差距最多 1 分）沒有結果，不能視為通過。

## 檔案與版本

| 項目 | 位置／目前版本 |
| --- | --- |
| 評分提示與結果驗證 | `backend/src/radar_scoring.rs`（`build_request`、`parse_scores`、`score`） |
| 提示版本 | `PROMPT_VERSION`：`radar-scoring-v1`；後端整合時寫入 `conversation_radar_results.rules_version` |
| 評估 runner | `backend/examples/try_f24_1.rs` |
| 評估程式版本 | `EVALUATOR_VERSION`：`f24.1-eval-v1` |
| 固定情境 | `backend/eval/f24_1_cases.json`（7 場對話，全為合成資料） |

修改提示或驗證規則時更新 `PROMPT_VERSION`；修改評估邏輯（穩定性、範圍判定）時更新 `EVALUATOR_VERSION`。

## 兩種檢查要分開看

| 檢查 | 用什麼 | 證明什麼 |
| --- | --- | --- |
| 契約測試 | 測試替身（`tests/radar_scoring.rs`、`radar_scoring.rs` 的單元測試） | 完整六維才採用；缺漏、重複、未知維度、非整數、超出範圍、無法解析一律拒絕整份結果；供應商失敗或逾時不產生分數；每次請求只帶該場對話 |
| 固定情境評估 | 真實模型（`cargo run --example try_f24_1`） | 模型依準則評分是否合理、重複評分是否穩定 |

契約測試通過不代表模型評分合理。

## 固定情境集

同一題（電車難題）與同一組六個維度，評分準則為繁體中文。

| 題號 | 涵蓋 | 設計重點 |
| --- | --- | --- |
| `OUTCOME_ZH` | 證據明確、對話隔離 | 學生始終以人數與結果為理由並表示願意負責 |
| `PRINCIPLE_ZH` | 證據明確、對話隔離 | 同一題、立場相反；與 `OUTCOME_ZH` 的分數方向應相反 |
| `NO_EVIDENCE_ZH` | 證據不足 | 學生幾乎沒有表達；六個維度都應為 0，且六個都要回傳 |
| `REVISED_ZH` | 修正立場 | 經追問後明確改變看法；依修正後的看法評分 |
| `VERIFY_EN` | 多語 | 英文對話搭配中文準則 |
| `EMPATHY_ES` | 多語 | 西班牙文對話搭配中文準則 |
| `FACILITATOR_ONLY_ZH` | 引導者的話不算證據 | 引導者提到感受與不確定，學生沒有接續 |

無效結果（缺漏、重複、未知維度、非整數、超出範圍、無法解析）由契約測試以測試替身涵蓋，不放在真實模型情境裡。

每題的 `expected` 是評審預期的分數範圍 `[最低, 最高]`，未列出的維度不設限。這是評審參考，不是團隊約定的驗收門檻。

## 執行方式

從 `backend/` 執行：

```bash
cargo test --example try_f24_1   # 離線檢查情境集，不呼叫 API
cargo run --example try_f24_1    # 真實模型評估，會使用 API 額度
```

真實模型評估使用已設定的 `OPENAI_MODEL` 與 OpenAI 金鑰（`.apikeylist` 或 `OPENAI_API_KEYS`），必要時加 `OPENAI_BASE_URL`。具體模型沿用部署設定，不在本評估定案。

| 變數 | 預設 | 說明 |
| --- | --- | --- |
| `EVAL_REPEATS` | 3 | 每題重複評分的次數（至少 2） |
| `CASE` | 未設定 | 只跑指定題號，例如 `CASE=REVISED_ZH` |

每題輸出一筆 JSON 到標準輸出，最後一筆是整輪摘要：

| 欄位 | 說明 |
| --- | --- |
| `runs` | 每次有效的六維分數 |
| `refused_or_failed_runs` | 被拒絕或失敗的次數（無效輸出、逾時、供應商錯誤） |
| `spread` | 各維度最高分減最低分 |
| `stable` | 每次都有效，且每個維度的 `spread` 不超過 1（S-04.2） |
| `outside_expected` | 有任何一次落在預期範圍外的維度 |
| `in_range` | 每次都有效，且沒有維度落在預期範圍外 |
| `qualitative_review` | 固定為 `pending`，由人工複核後另行記錄 |

輸出只含題號、分數與版本，不含對話內容。

## 評估紀錄

尚無。執行後在此登記日期、模型、`PROMPT_VERSION`、`EVALUATOR_VERSION`、穩定與落在範圍內的題數，以及人工複核結論。
