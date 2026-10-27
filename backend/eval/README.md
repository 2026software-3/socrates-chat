# F-07.1-AI 追問行為評估

對應 [Issue #183：追問類型選擇：AI 提示與行為評估](https://github.com/2026software-3/socrates-chat/issues/183)。專案規則以 `docs/specs/` 的 S-03.2、S-03.4 為準；本文記錄目前評估實作，不新增或取代產品規格。

## 評估目的

固定同一組學生對話情境，觀察修改提示詞後的追問行為。固定的是輸入與評分準則，不要求模型每次產生相同句子，也不把單一預設追問策略當成唯一正確答案。

目前分開檢查：

| 面向 | 執行者 | 判定內容 |
| --- | --- | --- |
| 格式完整性 | Rust 程式 | 正文、META 與必要 JSON 欄位是否完整合法 |
| 標籤一致性 | 評審獨立分類，再由程式比對 | `question_type` 是否符合實際追問的思考操作 |
| 策略適切性 | 評審模型 | 是否承接學生內容、有助於探究，並符合基本追問規則 |

例如：測資偏好 `reason`，但模型實際提出合理的概念澄清問題並標記 `clarify`，仍可通過；反之，即使命中測資預期標籤，只要與實際問題不一致，仍應失敗。

## 檔案與版本

| 項目 | 位置／目前版本 |
| --- | --- |
| 受測提示詞 | `backend/src/ai.rs` 的 `build_system_prompt` |
| 提示詞版本 | `rules_version(language)`：`v1.12/zh-TW`、`v1.12/en`、`v1.12/es`（v1.12 加入依對話語言輸出的三語提示與版本後綴；下方紀錄為 v1.10 的結果，v1.11／v1.12 尚未用真實模型重新評估，三種語言都是） |
| 評估 runner | `backend/examples/try_f07.rs`（`--lang` 選擇語言，預設 `zh-TW`） |
| 評估程式版本 | `EVALUATOR_VERSION`：`f07-eval-v5`（v5 加入評估語言） |
| 評審提示與準則 | runner 的 `JUDGE_PROMPT`（只有繁體中文版；三種語言共用，見下方限制） |
| 評審準則版本 | `JUDGE_VERSION`：`f07-rubric-v1/zh-TW` |
| 結果格式版本 | `schema_version: 5`（新增 `language` 欄位） |
| 固定情境 | `backend/eval/f07_1_cases_reviewed.json`（zh-TW）、`f07_1_cases_en.json`、`f07_1_cases_es.json` |

修改受測提示詞時更新 `RULES_VERSION`；修改評審準則時更新 `JUDGE_VERSION`；修改評估邏輯時更新 `EVALUATOR_VERSION`。結果欄位有不相容變更時更新 `schema_version`。

## 執行方式

### 1. 設定環境

在專案根目錄建立 `.env`（runner 讀取 `backend/../.env`）。以下為已使用的測試設定。金鑰不寫在 `.env`，改放專案根目錄的 `.apikeylist`（一行一把，格式見 `.apikeylist.example`；請求會輪流使用多把 key）：

```dotenv
OPENAI_MODEL=gpt-4.1-mini
OPENAI_JUDGE_MODEL=gpt-4.1-mini
# 選填；未設定時使用 https://api.openai.com/v1
OPENAI_BASE_URL=https://api.openai.com/v1
```

受測與評審使用同一個 API key、base URL；模型名稱可分別設定。`.env` 不加入版本控制。目前即使已設定 shell 環境變數，runner 仍要求根目錄 `.env` 可讀取；既有環境變數不會被 dotenvy 覆寫。

### 2. 跑離線測試

在 `backend` 目錄執行：

```powershell
cargo test --example try_f07
```

目前包含 13 個測試函式，涵蓋格式驗證、不同策略不直接判錯、命中預設標籤不代表一致、評審格式驗證，以及 FAIL／UNCERTAIN 不得算通過；另有語言參數解析、三份情境檔都通過驗證、en／es 情境與已審閱的 zh-TW 集逐題對應（題號、階段、標註、對話角色順序一致，且沒有殘留中文），以及系統提示依評估語言產生。這些測試不呼叫 API，也不驗證模型的實際生成品質。

### 3. 跑真實模型評估

```powershell
cargo run --example try_f07                 # 繁體中文（預設）
cargo run --example try_f07 -- --lang en    # English
cargo run --example try_f07 -- --lang es    # Español
```

`--lang` 接受 `zh-TW`、`en`、`es`（也可寫成 `--lang=en`）；其他值或參數會在呼叫 API 前報錯。語言決定使用哪份情境檔，以及受測系統提示的語言（對應 `rules_version(language)`）；每題結果與 `summary.json` 都會記錄 `language`。

runner 先驗證整份情境資料，再逐題執行受測模型；只有格式合格的回答才呼叫評審。15 題皆格式合格時，共呼叫受測模型 15 次、評審 15 次，會使用 API 額度；三種語言各跑一輪就是三倍。

每次呼叫預設首個 token 15 秒、完整回覆 60 秒的逾時。runner 不自動重試失敗案例。每次執行建立新的結果目錄，不覆寫舊輪次。

#### 調整逾時與題間間隔（選填）

下列環境變數可直接放在指令前，不必寫進 `.env`；未設定時行為與以前相同。

| 變數 | 預設 | 說明 |
| --- | --- | --- |
| `EVAL_FIRST_TOKEN_SECS` | 15 | 首個 content token 逾時（受測與評審共用）。推理型模型先輸出思考過程，`content` 長時間為空，實測可能要 7～13 秒以上，預設值會把整題判成 `API_ERROR` |
| `EVAL_TOTAL_SECS` | 60 | 完整回覆逾時（受測與評審共用） |
| `EVAL_DELAY_SECS` | 0 | 每題之間等待的秒數，用來避開免費供應商的限流 |

```bash
EVAL_FIRST_TOKEN_SECS=90 EVAL_TOTAL_SECS=240 EVAL_DELAY_SECS=20 cargo run --example try_f07
```

每題結果檔的 `timeouts_seconds` 會記錄該輪實際使用的值。比較不同模型時，請讓它們使用相同的逾時設定，否則比到的是速度而不是追問行為。

注意免費額度：OpenRouter 免費帳號所有免費模型合計每天只有 50 次請求（儲值 10 美元後為 1000 次），一輪完整評估約 30 次（受測 15、評審 15）。額度用完後每題都會是 `API_ERROR`，並且會**同時用掉使用同一把金鑰的其他服務的額度**。跑評估前先確認額度。

## 固定情境集

目前共 15 題，C、R、A、X、V 各 3 題，分別以澄清、理由探詢、假設檢視、反例思考、觀點比較為設計重點；題號不是強制輸出標籤。

每題包含以下欄位：

| 欄位 | 用途 |
| --- | --- |
| `id` | 唯一案例識別碼，例如 `R03` |
| `stage` | 目前階段，1～3 |
| `title`、`description` | 討論題目與說明 |
| `conversation` | 固定歷史訊息；角色為 `user` 或 `assistant` |
| `expected_type` | 原始標註的首選策略，僅供參考 |
| `acceptable_types` | 原始標註的可接受策略集合，僅供參考 |
| `learning_goal`、`annotation_reason` | 案例設計目標與標註說明 |

資料集不得為空；ID 不可重複；對話與學習目標不得為空；首選類型必須包含於可接受類型集合，集合不可重複。輸入標註目前限五種探究類型；輸出格式驗證另外接受 `wrap_up`，但目前 15 題未專門覆蓋收尾。

每種語言一份情境檔，只能用 `--lang` 切換；沒有指定任意檔案路徑的功能。

`f07_1_cases_en.json`、`f07_1_cases_es.json` 是已審閱的 zh-TW 集的逐題翻譯：題號、階段、`expected_type`、`acceptable_types`、對話角色順序都不變，只翻譯題目、說明、對話、`learning_goal` 與 `annotation_reason`。**翻譯與其在該語言的自然程度尚未經教師或母語者審閱**，在審閱前不應當作已校準的驗證集；離線測試只檢查結構與對應關係，不檢查譯文品質。

## 評估流程與準則

### 格式驗證

預期受測輸出：

```text
你所說的「公平」具體是什麼意思？
<<<META>>>
{"question_type":"clarify","advance":false,"reason":"學生尚未說明公平的意思。"}
```

程式要求正文非空、包含 META，且分隔標記後可完整解析為單一 JSON 物件，不接受 Markdown 包裝或額外文字。

- `question_type`：六種合法類型之一。
- `advance`：明確的 JSON 布林值，不接受缺省或字串。
- `reason`：非空字串，用於說明晉級判斷。

目前不拒絕額外 JSON 欄位，也不以程式嚴格檢查 META 是否獨占一行。此處為必要結構驗證，不代表提示詞所有規則都已被機械驗證。

### 獨立分類與比對

評審收到題目、階段、原對話與實際追問；不收到模型宣告的 `question_type`、`advance`、`reason`，也不收到測資的預期類型、學習目標或標註理由。

| 類型 | 主要思考操作 |
| --- | --- |
| `clarify` | 說清楚原本的詞義、指涉或立場範圍 |
| `reason` | 提供支持立場的理由 |
| `assumption` | 檢查已有前提、概括或推論是否可靠 |
| `counterexample` | 提出或尋找能測試原則的例外情境 |
| `perspective` | 比較不同立場，權衡價值、利益或取捨標準 |
| `wrap_up` | 整理、回顧既有討論 |

必要時參考前文辨認追問對象，不以階段或「為什麼」「標準」等單字直接分類。只有主要操作同時符合多類時，使用 `wrap_up > perspective > counterexample > assumption > reason > clarify` 的優先順序。

評審回傳 `CLEAR` 時只能給一種類型，程式比對後得到一致性 PASS 或 FAIL；回傳 `UNCERTAIN` 時可列一至兩種可能類型，一致性保留為 UNCERTAIN，不自動通過。這些是評審的判斷，並非經校準的信心機率。

### 策略適切性

評審依以下規則回傳 PASS、FAIL 或 UNCERTAIN：

- PASS：承接學生內容，有意義地釐清或探究；只有一個主要問題；中立尊重；不替學生下結論；必要時帶回主題。
- FAIL：有明確問題，例如機械重複已回答的內容、無關追問、多個獨立操作、灌輸答案或貶低學生。
- UNCERTAIN：資訊不足或有實質歧義。

合理策略不因另一種策略可能更好就失敗。這項評估衡量對話中的適切性，不直接計算原始 `learning_goal` 的達成率。

## 結果判讀

| 最終 `status` | 意義 |
| --- | --- |
| `JUDGE_PASS` | 格式合格，一致性與適切性均為 PASS |
| `JUDGE_FAIL` | 評審結果有效，但一致性或適切性至少一項 FAIL |
| `REVIEW_REQUIRED` | 沒有 FAIL，但至少一項 UNCERTAIN，需人工確認 |
| `FORMAT_ERROR` | 受測輸出格式不合法，跳過評審 |
| `API_ERROR` | 受測模型呼叫失敗或逾時，跳過評審 |
| `JUDGE_ERROR` | 評審呼叫失敗，或評審輸出格式不合法 |

FAIL 優先於 UNCERTAIN。`judge.status: OK` 只表示評審呼叫與解析成功，不表示受測回答通過。

`annotation_preferred_match`、`annotation_allowed_match` 只是輸出標籤與原始標註的比對，不參與通過判定。`NOT_REVIEWED`／`NOT_EVALUATED` 都不代表 PASS；評審未執行或失敗時應同時查看頂層 `status`。

執行完成後查看 `backend/eval/results/<run_id>/`：

| 檔案 | 內容 |
| --- | --- |
| `cases.json` | 該輪使用的情境檔快照 |
| `judge_prompt.txt` | 該輪使用的完整評審提示 |
| `01.json`～`15.json` | 逐題輸入、系統提示、原始輸出、解析結果、評審輸入與理由 |
| `summary.json` | 模型／版本、各狀態數量、比例及逐題索引 |

重要比例的分母不同：

- `format_valid_rate_all_cases`：格式合格數／全部案例。
- `judge_pass_rate_all_cases`：JUDGE_PASS 數／全部案例。
- `label_consistency_rate_judged_cases`：一致性 PASS 數／有效評審案例數。
- `strategy_appropriateness_rate_judged_cases`：適切性 PASS 數／有效評審案例數。

有效評審包含 FAIL 與 UNCERTAIN；格式、API 或評審錯誤不在這兩個語意比例的分母中。沒有有效評審時，比例為 `null`。`all_cases_judge_pass` 只是該輪是否全數自動通過，不代表 issue 已驗收。

runner 即使產生 JUDGE_FAIL 或 FORMAT_ERROR，正常完成寫檔後仍可能以成功狀態結束；目前不是用 process exit code 阻擋 CI 的驗收閘門。

## 已知實測紀錄（2026-10-09）

以下根據已提供的終端摘要與逐題 JSON 記錄，不推估未提供的統計。受測及評審模型均為 `gpt-4.1-mini`，評估程式／準則為 `f07-eval-v4`／`f07-rubric-v1/zh-TW`。

| Run ID | 提示詞 | 已確認結果 | 證據範圍 |
| --- | --- | --- | --- |
| `1791534239018100500` | `v1.9/zh-TW` | 15 題中 12 筆 JUDGE_PASS、3 筆 FORMAT_ERROR；API 與評審錯誤均為 0 | 完整終端摘要；R03、X01、X03 原始輸出皆缺少 META／JSON |
| `1791534515167865400` | `v1.10/zh-TW` | 15／15 格式合格與 JUDGE_PASS；API 與評審錯誤均為 0 | 完整終端摘要；另有 R02 逐題 JSON |
| `1791535258640105500` | `v1.10/zh-TW` | 已確認 R03 為 JUDGE_FAIL：一致性 FAIL、適切性 PASS、格式合格 | 整輪統計PASS=13 FAIL=2 |

v1.10 將只含 JSON 的格式範例改成「追問＋META＋JSON」完整範例，並提醒模型完成 JSON 後才結束輸出。一輪格式全數通過是觀察結果，尚不足以證明漏 META 的根因或保證以後不再發生。

### 已知失敗：第二輪 R03

學生反對將志工服務列為畢業條件，但尚未想清楚理由。受測模型問：

> 你可以試著想想，強制學生做志工服務，可能會帶來哪些好處或壞處？哪一方面你覺得比較重要？

模型標記 `reason`；評審判為 `perspective`，因實際操作是比較利弊與重要性，所以一致性 FAIL。這筆即使命中測資預期的 `reason`，也不會被算成通過。

評審的適切性判為 PASS，但人工仍可檢查其先列利弊、再判斷重要性的問法是否包含多個獨立操作；自動 PASS 不替代人工判斷。本紀錄保留原始失敗，不修改結果或以重跑後的成功覆蓋它。

### 人工抽查觀察：第一輪 R02

追問「你認為在上課時完全禁止使用手機的標準是什麼？」被模型與評審共同分類為 `clarify`。此句亦可能被理解為索取支持禁令的依據，因此存在 `clarify`／`reason` 的語意歧義；評審標為 CLEAR 不表示歧義已被排除。這是抽查觀察，不覆寫原始自動評分，也不當作已完成教師審查。

## 人工複核與限制

人工複核依序查看學生對話、`parsed.response`、`parsed.question_type`，最後才讀 `judge.parsed` 的理由。以獨立紀錄保存判斷，避免更動原始 JSON。建議記錄 run ID、case ID、一致性、適切性、理由與複核者／日期。

目前限制：

- 生成模型與評審是同一型號，雖然分開呼叫且評審看不到模型標籤，仍可能有共同盲點。
- 15 題已用於提示詞調整，屬開發評估集，不是未見過的獨立驗證集；結果不能直接推論到所有學生對話。
- 相同設定重跑可能產生不同回答與判斷。本 runner 未指定 temperature、seed 或固定模型快照，模型名稱也不代表完全可重現。
- 未評估 `advance` 判斷的語意正確性；只驗證它是布林值、`reason` 是非空字串。未驗證完整多回合階段推進與收斂。
- en／es 已有情境集與 runner 支援，但還沒有任何真實模型的評估紀錄；v1.12 三種語言的行為都尚未實測。
- 評審提示與準則只有繁體中文版，三種語言共用：評審用中文準則判讀英文／西班牙文的對話與追問。這未經驗證，可能與各語言的評審有系統性差異；結果比較三種語言時要考慮這一點。
- 一個主要問題、尊重中立等要求目前由評審綜合評估，沒有各自獨立的硬性程式檢查。
- 未保存 API `finish_reason`；現有紀錄不足以完全區分模型自行停止與輸出遭截斷。串流／逾時錯誤時 `collect` 不回傳部分輸出。
- 教師團隊定期人工抽查仍需另行安排；目前不能把此文件視為教師核准或 issue 完成證明。

## 提交與維護

修改提示或準則後，執行離線測試與固定情境評估，保存每輪版本與結果；遇到失敗先讀原文及評審理由，保留失敗樣本。

PR 建議附 runner、提示詞變更、固定情境、本文，以及選定的評估紀錄與人工複核說明。只使用合成情境，勿提交 `.env`、API key 或真實學生對話。若結果目錄被忽略，可依團隊流程另附評估證據，不必提交所有暫存輪次。

目前保留 `v1.10` 作為提交評估的候選版本；已知有一輪全數自動通過，也有後續分類失敗。行為驗收門檻與是否接受這些限制由團隊審查確認，不因單輪 15／15 或單純容忍偶發錯誤而宣告完成。離線測試的實際執行結果尚未收錄於本紀錄。
