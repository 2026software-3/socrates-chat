# F-10.1-AI 語音逐字稿處理評估

對應 [Issue #200：語音輸入確認模式：AI 提示與行為評估](https://github.com/2026software-3/socrates-chat/issues/200)。規格依據為 `docs/specs/` 的 S-05.1、S-05.3、S-05.4 與 S-03.4；本文記錄評估方式，不新增或取代產品規格。

## 要解決的問題

S-05.1 規定語音辨識文字由學生確認後才送出，但學生確認後的逐字稿仍常帶有同音錯字、贅詞、重複與自我修正（例如「應該要拉感」「效益主意」）。若 AI 把這些當成學生的真正用詞，可能會糾正錯字、對「拉感」追問詞義，或讓學生覺得被挑錯。

規格沒有定義 AI 該如何看待語音逐字稿；以下做法是本 Issue 的實作選擇，**需經團隊 review 確認**：

- 後端把 `source` 為 `speech-browser`、`speech-openai` 的學生訊息，在送往 AI 的請求中加上 `【語音輸入】` 標記（`ai::student_turn_content`）。資料庫與學生看到的內容維持原文，標記只存在於出站請求。
- 系統提示（`RULES_VERSION` `v1.11/zh-TW`）說明標記的意義，要求 AI：依最合理原意理解、不糾正或評論錯字與贅詞、不要求改用文字；只有辨識錯誤讓關鍵詞或立場出現明顯不同的理解時，才用一個簡短問題請學生確認（標記 `clarify`）；不提到標記本身；`advance` 不因口語化而改變。
- 標記用全形括號 `【】`，避免和 S-02.6 的個資佔位詞（`[EMAIL]` 等方括號大寫詞）混淆。

## 檔案與版本

| 項目 | 位置／目前版本 |
| --- | --- |
| 受測提示詞 | `backend/src/ai.rs` 的 `build_system_prompt`（「語音輸入」段落） |
| 語音標記 | `ai::SPEECH_MARK`、`ai::student_turn_content` |
| 提示詞版本 | `RULES_VERSION`：`v1.11/zh-TW` |
| 評估 runner | `backend/examples/try_f10_1.rs` |
| 評估程式版本 | `EVALUATOR_VERSION`：`f10_1-eval-v1` |
| 評審準則版本 | `JUDGE_VERSION`：`f10_1-rubric-v1/zh-TW` |
| 結果格式版本 | `schema_version: 1` |
| 固定情境 | `backend/eval/f10_1_cases.json`（11 題，全為合成資料） |

## 執行方式

設定方式與 F-07.1 相同（見 [`README.md`](README.md) 的「設定環境」）：在專案根目錄 `.env` 填入 `OPENAI_API_KEY`、`OPENAI_MODEL`、`OPENAI_JUDGE_MODEL`。

```bash
cd backend
cargo test --example try_f10_1   # 離線測試，不呼叫 API
cargo run --example try_f10_1    # 真實模型評估：受測 11 次＋評審最多 11 次，會使用 API 額度
```

結果寫在 `backend/eval/results/f10_1-<run-id>/`，每題一個 JSON 與 `summary.json`，不覆寫舊輪次。

## 固定情境集

| 類別 | 題號 | 設計重點 |
| --- | --- | --- |
| 雜訊不影響原意 | N01–N06 | 同音錯字（拉感、效益主意、康得、依務論）、贅詞、重複、自我修正；應直接承接論點，不糾正 |
| 原意有實質分歧 | A01–A03 | 否定詞斷開（沒／有責任）、同音詞指向不同意思（有力＝有理／有利）、辨識中斷；應簡短請學生確認 |
| 文字對照 | T01–T02 | 純文字、語音與文字混合；不應無故提到語音 |

`source` 涵蓋 `text`、`speech-browser`、`speech-openai`。`ambiguity`（`none`／`material`）是標註者的參考，評審看不到。

## 評估流程

1. **程式檢查**：輸出格式（正文、`<<<META>>>`、JSON 欄位）與 F-07.1 相同；另檢查正文不得出現 `【語音輸入】`，外洩直接記為 `MARK_LEAK`，不送評審。
2. **評審模型**：看得到每則學生訊息的 `source`，看不到標註。先獨立判斷 `meaning_ambiguity`（NONE／MATERIAL／UNCERTAIN），再評：
   - `speech_handling`：是否依原意承接、不糾正錯字與口語、該確認時確認、不該確認時不做多餘確認、不提到標記。
   - `strategy_appropriateness`：只有一個主要問題、中立尊重、不代答。
3. **合併**：任一 FAIL → `JUDGE_FAIL`；任一 UNCERTAIN → `REVIEW_REQUIRED`；全部 PASS → `JUDGE_PASS`。評審判斷與標註的 `ambiguity` 不同時只記在 `annotation_ambiguity_match`，不直接判失敗。

## 評估紀錄

| Run ID | 提示詞版本 | 模型／評審 | 結果 |
| --- | --- | --- | --- |
| （待補） | `v1.11/zh-TW` | （待補） | （待補） |

提交 PR 前請至少跑一輪真實模型評估並補上本表；失敗樣本保留原始紀錄，不以重跑的成功覆蓋。

## 限制

- 情境是打字模擬的逐字稿，不是實際音訊辨識結果；不評估 OpenAI 語音辨識（STT）本身的準確度。
- 「何時算實質分歧」本身有主觀成分，評審與標註可能不一致，需要人工抽查 A 類與評審判 MATERIAL 的題目。
- 只評估單次回覆，不評估多回合下的階段推進，也未評估 `advance` 的語意正確性。
- 只有 `zh-TW` 情境；英文與西班牙文尚未涵蓋（S-12.1 仍待決定評估集是否三語各一套）。
- 自動評分不代表 Issue 已通過團隊驗收。
