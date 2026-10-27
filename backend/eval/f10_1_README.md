# F-10.1-AI 語音逐字稿與錯字理解評估

對應 [Issue #200：語音輸入確認模式：AI 提示與行為評估](https://github.com/2026software-3/socrates-chat/issues/200)。規格依據為 `docs/specs/` 的 S-05.1、S-05.3、S-05.4 與 S-03.4；本文記錄評估方式，不新增或取代產品規格。

## 要解決的問題

S-05.1 規定語音辨識文字由學生確認後才送出，但確認後的逐字稿仍常帶有同音錯字、贅詞、重複與自我修正（例如「應該要拉感」「效益主意」）。打字也一樣會有錯字。若 AI 把這些當成學生的真正用詞，可能會糾正錯字、對「拉感」追問詞義，或讓學生覺得被挑錯。

依 #334 review 的建議，**語音與打字一律當作文字處理，不另加來源標記**：

- 後端送往 AI 的內容與資料庫保存的原文相同，不區分來源（`reply.rs` 不變）。
- 系統提示（`v1.13`，繁中、英文、西班牙文三份提示詞都加入同樣的規則，符合 S-12.1「追問準則在三種語言下一致」）新增「理解學生的訊息」段落，對所有學生訊息生效：依最合理原意理解、不糾正或評論錯字與贅詞、不要求改用其他方式輸入；只有錯字讓關鍵詞或立場出現明顯不同的理解時，才用一個簡短問題請學生確認（標記 `clarify`）；`advance` 不因口語化、錯字或贅詞而改變。

## 檔案與版本

| 項目 | 位置／目前版本 |
| --- | --- |
| 受測提示詞 | `backend/src/ai.rs` 的 `build_system_prompt`（「理解學生的訊息」段落） |
| 提示詞版本 | `rules_version(language)`：`v1.13/zh-TW`、`v1.13/en`、`v1.13/es` |
| 評估 runner | `backend/examples/try_f10_1.rs` |
| 評估程式版本 | `EVALUATOR_VERSION`：`f10_1-eval-v2` |
| 評審準則版本 | `JUDGE_VERSION`：`f10_1-rubric-v2/zh-TW` |
| 結果格式版本 | `schema_version: 2` |
| 固定情境 | `backend/eval/f10_1_cases.json`（12 題，全為合成資料） |

## 執行方式

API key 放在專案根目錄的 `.apikeylist`（一行一把，見 `.apikeylist.example`），或用 `OPENAI_API_KEYS`；`.env` 另需 `OPENAI_MODEL`、`OPENAI_JUDGE_MODEL`，必要時加 `OPENAI_BASE_URL`。

```bash
cd backend
cargo test --example try_f10_1   # 離線測試，不呼叫 API
cargo run --example try_f10_1    # 真實模型評估：受測 12 次＋評審最多 12 次，會使用 API 額度
```

免費或推理型模型較慢、常被限流時，可與 F-07.1 一樣用環境變數調整：`EVAL_FIRST_TOKEN_SECS`、`EVAL_TOTAL_SECS`（預設 15／60 秒）、`EVAL_DELAY_SECS`（題與題之間暫停，預設 0）。

結果寫在 `backend/eval/results/f10_1-<run-id>/`，每題一個 JSON 與 `summary.json`，不覆寫舊輪次。

## 固定情境集

| 類別 | 題號 | 設計重點 |
| --- | --- | --- |
| 雜訊不影響原意 | N01–N06 | 語音逐字稿的同音錯字（拉感、效益主意、康得、依務論）、贅詞、重複、自我修正；應直接承接論點，不糾正 |
| 原意有實質分歧 | A01–A03 | 否定詞斷開（沒／有責任）、同音詞指向不同意思（有力＝有理／有利）、辨識中斷；應簡短請學生確認 |
| 文字對照 | T01–T03 | 純文字、語音與文字混合，以及打字時的同音錯字（T03）；規則對打字同樣生效 |

情境中的 `source`（`text`、`speech-browser`、`speech-openai`）只是情境說明，**送給受測模型的內容不含來源**，與正式系統相同。`ambiguity`（`none`／`material`）是標註者的參考，評審看不到。

## 評估流程

1. **程式檢查**：輸出格式（正文、`<<<META>>>`、JSON 欄位）與 F-07.1 相同。
2. **評審模型**：看得到每則學生訊息的 `source`（受測模型看不到），看不到標註。先獨立判斷 `meaning_ambiguity`（NONE／MATERIAL／UNCERTAIN），再評：
   - `input_handling`：是否依原意承接、不糾正錯字與口語、該確認時確認、不該確認時不做多餘確認、不無故提到輸入方式。
   - `strategy_appropriateness`：只有一個主要問題、中立尊重、不代答。
3. **合併**：任一 FAIL → `JUDGE_FAIL`；任一 UNCERTAIN → `REVIEW_REQUIRED`；全部 PASS → `JUDGE_PASS`。評審判斷與標註的 `ambiguity` 不同時只記在 `annotation_ambiguity_match`，不直接判失敗。

## 評估紀錄

| Run ID | 提示詞版本 | 模型／評審 | 結果 |
| --- | --- | --- | --- |
| （待補） | `v1.13/zh-TW` | （待補） | （待補） |

提交 PR 前請至少跑一輪真實模型評估並補上本表；失敗樣本保留原始紀錄，不以重跑的成功覆蓋。

## 限制

- 情境是打字模擬的逐字稿，不是實際音訊辨識結果；不評估語音辨識（STT）本身的準確度。
- 「何時算實質分歧」本身有主觀成分，評審與標註可能不一致，需要人工抽查 A 類與評審判 MATERIAL 的題目。
- 只評估單次回覆，不評估多回合下的階段推進，也未評估 `advance` 的語意正確性。
- 只有 `zh-TW` 情境；英文與西班牙文尚未涵蓋（S-12.1 仍待決定評估集是否三語各一套）。
- 自動評分不代表 Issue 已通過團隊驗收。
