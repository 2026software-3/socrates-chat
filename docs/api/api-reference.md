# 後端 API 說明

> 端點清單、路徑、參數與請求／回應欄位**不寫在這裡**，以 OpenAPI 3 為準：後端執行時公開在 `GET /api/openapi.json`，由各 handler 的 `#[utoipa::path]` 註解與型別的 `ToSchema` 產生（新增或修改端點時要同步改註解；CI 會失敗的檢查有兩個：`backend/tests/openapi.rs` 檢查已文件化的端點都有路由且受保護；`backend/tests/openapi_coverage.rs` 掃描原始碼的 `.route(`，路由漏標註或標註沒有對應路由都會失敗）。
> 渲染後可瀏覽的 API reference 在 `GET /api/docs`（Redoc，公開、不需登入，內容由同一份 OpenAPI 即時產生；Redoc 的 JS 由後端自己提供，不依賴外部 CDN）。頁面的色票與字型取自 Figma wireframe 的 Style Guideline，設定在 `backend/src/redoc.html`。
> 這份文件只補 OpenAPI 寫不下的**規則與流程**。它不是規格，規則以 [`docs/specs/`](../specs/decisions.md) 為準。
> 所有路徑在同一網域的 `/api` 之下（S-08.1）。認證靠 `sid` HttpOnly cookie。

## 通則

- **CSRF**：`POST`／`PUT`／`PATCH`／`DELETE` 必須帶 `Origin` 標頭，且等於 `APP_BASE_URL`，否則 `403 csrf`（瀏覽器會自動帶）。
- **錯誤格式**：`{ "error": { "code": "...", "request_id": "..." } }`；只有 `code`，沒有已翻譯文字，前端依 `code` 顯示（S-12.1）。回應標頭也有 `x-request-id`。
- **常見錯誤碼**：`unauthorized`（401）、`forbidden`（403）、`account_disabled`（403，帳號已被管理者停用）、`password_change_required`（403，用臨時密碼登入尚未改密碼）、`not_enrolled`（403，名單外帳號）、`not_found`（404）、`csrf`（403）、`internal`（500）。各端點特有的錯誤碼寫在 OpenAPI 的 responses 說明。
- **PATCH 語意**：沒帶的欄位保留原值；可為空的欄位（題目的 `category`）送 `null` 代表清除（JSON merge-patch，RFC 7396）。`description` 不可為 `null`，要清空請送 `""`。
- **權限分層**：管理者專用的在 `/api/admin`；教師或管理者；只有修課名單內的學生。對話內容只有擁有者讀得到，別人（含教師、管理者）一律 `404`。

## 登入

- 沒有註冊 API：內建帳號只由系統建立（管理者重設密碼、管理者匯入修課名單、部署時的 `ADMIN_INITIAL_PASSWORD`）。
- 內建登入失敗一律回同一個 `invalid_credentials`（帳號不存在與密碼錯誤相同）；同一信箱從同一個 IP 連續失敗 5 次鎖定 15 分鐘，期間回 `429 too_many_attempts`（別的 IP 的少量失敗不會鎖住本人）；同一信箱不論 IP 累計失敗 30 次（暫定值）則在所有 IP 鎖定 15 分鐘；另外每個來源 IP 有請求頻率限制（連續 60 個、之後平均每秒 2 個，暫定值），超過同樣回 `429 too_many_attempts`。
- Google 回呼成功設定 `sid` cookie 並導回 `APP_BASE_URL`；失敗導回 `/login?error=<code>`（`invalid_state`、`email_not_verified`、`access_denied`、`login_failed`）。
- 用臨時密碼登入時 `must_change_password` 為 `true`：前端導向改密碼頁，除了 `GET /api/me`、改密碼與登出，其他 API 都回 `password_change_required`。旗標在 session 上，不在使用者上。
- 改密碼成功後，該使用者**其他** session 失效（保留目前這個）。新密碼長度 8–128。
- `GET /api/me` 的 `roles` 全為 `false` 代表「尚未開通」。教師與管理者不需要在修課名單內。

## 管理（管理者）

- 重設密碼：臨時密碼**只出現在這次回應**；沒有帳號就建立；同時清除該使用者所有 session 與登入鎖定。
- `PUT /api/admin/users/{id}/status` 接受 `{ disabled, reason? }`，id 為使用者 UUID（學生列表的 `id`，沒有帳號時為 null）；舊 disable／enable POST 已移除。同值重送回 204 且不重複稽核。停用帳號：立即清除該帳號所有 session，資料保留；不能停用自己（`cannot_disable_self`），系統至少保留 1 位啟用中的管理者（`last_admin`）；停用與復原都寫入稽核日誌。
- 新增教師重複送出也回 204（冪等）。

## 題庫與活動

- 教師和管理者統一使用 `GET /api/topics`、`POST /api/topics`、`PATCH /api/topics/{id}`；原 `/api/admin/topics` 的三種方法已移除（#292–#294）。
- 題目停用後學生看不到。系統啟動時會補上內建題目「電車難題」（固定 ID；已存在就不動，教師改過或停用後也不會被覆寫）。
- 活動建立時是 `draft`；`publish` 後學生才選得到，可重新開放；`close` 停止開放。
- 活動必須選題庫題目（S-04.7）：建立時 `topic_id` 必填，沒帶、為 `null` 或題目不存在都回 `400 invalid_topic`，`PATCH` 也不能清除。活動已有學生開始的對話後，換成不同題目回 `409 activity_topic_locked`（同一個題目、只改標題或說明不受限；對話被刪除後又可更換）。
- 活動屬於教室（F-28.3、S-14）：`POST /api/activities` 本文必須帶 `classroom_id`（沒帶回 `400 invalid_classroom`），建立後不可更改（`PATCH` 沒有這個欄位）。教室不存在與不是自己管理的教室一律回相同的 `404 not_found`；教師只能建立、讀取、修改、發布與停止自己建立的教室內的活動（其他教師的活動回 `404`），管理者可管理所有教室。題目是全域的：兩個教室可以引用同一題目，各教室的活動互不影響。
- `GET /api/activities?classroom_id=` 列出自己管理的教室內的活動（管理者為全部），回應每筆都有 `classroom_id`；帶 `classroom_id` 時只看那間教室，不是自己管理的教室回 `404`。
- `GET /api/catalog` 回傳 `{ activities, topics }`，`activities` 只含已發布、且所在教室的名單內有這位學生的活動，`topics` 為已啟用的題目；舊的 `/api/available` 已移除。

## 修課名單與學生（F-28.2）

修課名單屬於教室（S-14、S-01.4）；名單端點的路徑都帶教室 ID，由教室的建立者或管理者操作。其他教師存取他人的教室，與教室不存在、ID 格式不正確一律回相同的 `404 not_found`；學生、名單外帳號回 `403`，未登入回 `401`。

- `GET /api/classrooms/{id}/roster`：該教室名單的電子郵件 `{ emails }`。
- `POST /api/classrooms/{id}/roster/import`：`{ text }` → `{ added, existing, invalid, credentials }`。名單只屬於該教室，不影響其他教室；`added`／`existing` 都是相對這間教室。同一電子郵件可同時在多個教室的名單，帳號仍只有一個。
- `DELETE /api/classrooms/{id}/roster/{email}`：只移出這間教室；原本就不在這間教室名單內回 `404`。學生仍在其他教室的名單時保有學生權限；不在任何教室名單內時，登入後看到 `403 not_enrolled`（尚未開通）。
- `GET /api/classrooms/{id}/students`：該教室名單上的學生與帳號狀態。`completed_conversations` 是該學生在這間教室內已完成的活動對話數，個人練習與其他教室的對話不計入。
- `GET /api/dashboard/class` 的 `students_total`：教師為自己建立的所有教室內不重複的學生數，管理者為全部教室。
- 舊的 `/api/roster*` 與 `/api/students`（列表）已移除；`DELETE /api/students/{email}` 仍是全域的帳號刪除，會一併移除該帳號在所有教室的名單項目。教師只能刪除完全屬於自己建立的教室的學生，其餘（含他人教室的學生、同時在他人教室的學生）回與不存在相同的 `404`；管理者不受限。此範圍是暫定的保守做法，**待需求方確認**（S-02.5，見 `decisions.md`「尚未決定」）。
- 匯入只新增、不移除；每行一個電子郵件，可有 `email` 標題列。
- 匯入結果的 `credentials` 是這次新建立的內建帳號與臨時密碼（只出現這一次）。**只有管理者匯入才會建立帳號**，教師匯入時為空陣列；已有帳號與教師／管理者的信箱不在其中。
- 移出名單不刪資料（重新加回後照舊）。
- 刪除學生帳號：使用 DELETE，JSON body 的 `confirm_email` 必須與路徑的電子郵件相同（`confirmation_mismatch`）；一併刪除對話、總結與名單項目；教師與管理者帳號不能由此刪除（`not_a_student`）；寫入稽核日誌。
- DELETE 刻意保留 JSON body，以沿用確認欄位與驗證規則；前端 fetch 與後端 Axum 均支援。部署的 proxy／client 必須保留 DELETE body。
- 學生列表只有完成對話的**數量**，沒有內容。

## 教室（F-28.1）

- 教師建立教室；同一位教師可建立多間。教室名稱必填，前後空白會去除，最多 255 字元，且不分大小寫全域唯一；重複名稱回 `409 classroom_name_taken`，空白或超長名稱回 `400 invalid_classroom_name`。年度標籤最多 100 字元、說明最多 2,000 字元，超限分別回 `invalid_classroom_year_label`、`invalid_classroom_description`。選填欄位前後空白會去除，空字串會保存為 `null`。
- `GET /api/classrooms`：教師只取得自己建立的教室，管理者取得全部教室。建立者與管理者可檢視／編輯教室；其他教師存取他人教室時與不存在的教室一律回相同 `404 not_found`。
- 管理者可管理所有教室。依 Issue「教室由教師建立」，目前只有教師可建立教室（管理者建立權限**待需求方確認**，尚未定案）；建立者欄位固定為建立請求的教師。教室名稱唯一性由資料庫約束保證，併發建立相同名稱時僅一個成功。沒有提供任何欄位的空 PATCH 不會更新 `updated_at`。
- PATCH 未提供的欄位保留原值；`year_label`、`description` 送 `null` 可清除。封存與活動歸屬尚未包含在此切片；名單見上一節。

## 對話（只有修課名單內的學生）

教師與管理者不參與討論：對話、可選清單、語音與 `/api/dashboard/me` 對教師與管理者一律回 `403 forbidden`（名單外帳號回 `not_enrolled`）。

- `GET /api/conversations?limit=&cursor=` 依 `created_at DESC, id ASC` 回 `{ items, next_cursor }`；預設 50 筆，limit 必須為 1–200 的整數，否則 400 `invalid_limit`。cursor 使用上一頁的非空 `next_cursor`，最後一頁為 null，損壞的 cursor 回 400 `invalid_cursor`。只查詢本人資料，不含訊息；前端可往前／往後翻頁。
- 開始對話的來源須為已發布活動或已啟用題目，否則 `400 invalid_conversation_source`。活動還必須在學生所屬教室的名單內：活動不存在、未發布與學生不在活動所屬教室名單內，回應完全相同，不外露活動是否存在。
- 對話自己保存 `classroom_id`（S-14），不以活動推得：**活動對話**取自活動所在的教室，建立後不可更改，活動之後被刪除或停用也不變；只帶 `topic_id` 的**個人練習**為 `null`，不屬於任何教室，教師看不到。學生只要仍在任一教室的名單內就能做個人練習。學生的 `GET /api/conversations` 同時包含兩種對話。
- 每位學生在每個活動至多一場對話，進行中或已結束都算（S-04.7）：已有時以該活動開始對話回 `409 activity_already_started`，同時送出的兩個請求也只會成功一個。刪除該場對話後可重新開始；只帶 `topic_id` 的個人練習不受限。
- 訊息 `source`：`text`、`speech-browser`、`speech-openai`（後兩者代表語音輸入的逐字稿，S-05.4）。長度上限 4000 字元（`invalid_message`）。
- **送出訊息即串流回覆（S-08.4）**：`POST …/messages` 回 `200 text/event-stream`，事件依序為：
  1. `message`：保存好的學生訊息，另有 `auto_ended`。
  2. `delta` `{text}`（多次），再 `done` `{message_id, question_type, stage, suggest_end}`；失敗則 `error` `{code}`（`ai_unavailable`＝顯示「AI 暫時無法回覆」與重試按鈕）。
  - `auto_ended` 為 `true`（達回合上限，對話已自動結束、總結產生中）時只有 `message` 事件，沒有 AI 回覆。
  - 保存前的失敗（`invalid_message`、`conversation_ended`、`reply_in_progress`…）是一般 JSON 錯誤，不是串流。
- **重試**：`GET …/stream`（SSE）只用於「最後一則是學生訊息、還沒有 AI 回覆」（上次串流失敗或斷線）；事件只有 `delta`／`done`／`error`，不會再保存學生訊息。已有回覆回 `409 nothing_to_reply`。
- 同一場對話同時只有一條回覆在產生：AI 回覆中不接受新訊息或第二條串流（`409 reply_in_progress`）。前端中途斷線時，後端仍會把完整回覆存下來。
- 結束對話：背景產生總結（首次 202；已結束 200）；沒有訊息時 `empty_conversation`；AI 還在回覆時 `reply_in_progress`。
- 總結：`status` 為 `pending`／`ready`／`failed`，`pending` 時請輪詢；失敗可用 `summary/retry` 重試（`summary_not_retryable`）。總結不可由學生修改。
  - `claim`：學生最終主張的一句短句（AI 寫的，約 30 字內，用對話語言，分組用）。
  - 固定學派的 `framework`（主要學派）與 `framework_scores`（6 維分數）已取消（S-04.6）。題目綁定的新維度與分數由後續功能（F-03.4、F-24）加入，屆時再更新本節。
- 刪除對話會一併刪除訊息與總結。

## 儀表板

論點分布的主體是「主張」：AI 為每場對話寫一句最終主張，同一個活動下再把意思相近的歸成一群並命名（轉變先不記，只有最終主張）。教師儀表板以活動為彙整單位（S-04.7）：每位學生在一個活動至多一場對話；從題目庫開始的個人練習不屬於任何教室，不列入。

- `GET /api/dashboard/me`：`{ total_conversations, total_turns, completed, recent }`，只算自己的資料；`recent` 是最近完成的 10 場（含主張與立場）。固定學派的雷達圖與學派人數已取消（S-04.6）。
- `GET /api/dashboard/class?classroom_id=`：`{ masked, students_total, students_participating, completed, activities }`；`activities` 每項為 `{ activity_id, title, completed, claims, ungrouped }`，完成場數多的在前。只計入自己管理的教室內的活動對話（管理者為全部），個人練習與他人教室一律不計入；帶 `classroom_id` 時只看那間教室（`students_total` 為該教室名單人數），不是自己管理的教室回 `404`。**沒帶 `classroom_id` 時涵蓋自己管理的所有教室，只是 F-28.4 加入教室切換前的暫時行為**；S-14 規定彙整範圍限單一教室，是否改為必填**待需求方確認**。
- 班上儀表板只給教師（S-02.4 不設最小群體門檻），只有數量與分類，沒有學生身分或內容。管理者身分一律 `masked: true` 且沒有分布（S-02.2）。
- 活動的 `claims`：人數多的在前；`reasons` 是這群人的核心理由（AI 總結的文字，最多 5 則、去重，沒有姓名與原文）；超過 6 群時最小的幾群併成 `other: true`（沒有名稱）；`ungrouped` 是有主張但還沒歸群的場數（分組失敗或尚未分組）。
- 重新分組：`POST /api/dashboard/class/regroup`，本文 `{ activity_id }`，教師請 AI 把該活動下全部主張重新分組。活動不存在或不在自己管理的教室回 `404`；自己教室的活動沒有可分組的主張回 `409 no_claims`；AI 失敗回 `502 ai_unavailable` 並沿用原本的分組；管理者 `403`。

## 語音備援（只有修課名單內的學生）

瀏覽器預設用 Web Speech API；不支援時才經 `/api/voice/*` 呼叫 OpenAI（S-05.3）。**不保存原始音訊**（S-05.4）。具體語音模型尚未決定，沒設定 `OPENAI_STT_MODEL`／`OPENAI_TTS_MODEL` 時回 `503 voice_unavailable`；外部服務失敗回 `502 voice_failed`。語音辨識的請求本文是音訊位元組（`Content-Type: audio/*`，上限 10 MB），`language` 為 `zh-TW`、`en` 或 `es`；合成文字 1–4000 字元。

## 教師檢視

教師檢視學生的總結**不含對話原文**（S-02.1）。`GET /api/teacher/summaries?classroom_id=` 只列出自己建立的教室內的活動對話總結（管理者為全部教室，內容遮蔽），每筆有 `classroom_id`；個人練習永遠不在其中。帶 `classroom_id` 時只看那間教室，不是自己管理的教室回 `404`。管理者身分的敏感欄位一律看到固定字串 `message`（S-02.2）。

## 規劃中的端點（尚未實作）

> 以下路徑是依 open issues 與規格整理的**提案**，不是規格；實作前要依 CONTRIBUTING 走規格決策流程。實作後以 OpenAPI 為準，並從這張表移除。

| Issue | 功能 | 建議路由 | 備註 |
| --- | --- | --- | --- |
| F-19.1/19.2/19.4、S-07.2 | 點數餘額與帳本 | `GET /api/points`（餘額）、`GET /api/points/ledger?cursor=` | 僅本人；發點在後端事件（總結 ready）內完成，不開放寫入 API |
| F-19.3、S-07.4 | 任務與領取 | `GET /api/tasks`（含進度、週期）、`POST /api/tasks/{id}/claim` | 領取冪等：重複領取要明確回 409，與規格「重複遭拒絕」一致 |
| F-17.1–17.3 | 部件、購買、外觀 | `GET /api/parts`、`POST /api/parts/{id}/unlock`、`GET·PUT /api/me/avatar` | 與兌換合併成同一購買流程（S-07 §1） |
| F-19.4 | 管理員定價 | `PATCH /api/admin/parts/{id}`、`PATCH /api/admin/tasks/{id}` | 須寫稽核日誌 |
| F-18 | 成就 | `GET /api/achievements` | 解鎖由後端事件觸發，無寫入 API；S-07.3 未定 |
| F-20 / S-07.6 | 成果分享 | `POST /api/shares`、`GET /api/shares/{token}` | 預設隱藏個資；格式未定 |
| F-12 | 邏輯樹 | `GET /api/conversations/{id}/logic-tree` | S-04.4 未定 |
| F-13 | 思辨標籤 | `GET /api/conversations/{id}/labels` | S-04.5 未定 |
| F-14 | 閱讀推薦、教師書單 | `GET /api/conversations/{id}/recommendations`、`GET·POST·DELETE /api/books` | S-06.3 未定 |
| F-10.3 | 雙方逐字稿同步 | 現有 `messages.source` 已部分涵蓋；是否需新端點待 F-10.3 釐清 | |
| F-27 / S-13 | 名詞解釋 | `GET /api/glossary?lang=` | 來源未定 |
| S-09 | 營運探針 | `GET /ready`（含 DB 檢查） | 目前只有 `/health` |

## 設計 audit 與待辦

這一節記錄對已實作 API 的設計檢視。嚴重度：**高**＝可能造成錯誤或資安問題；**中**＝一致性／可維護性；**低**＝建議。

**已具備**：錯誤格式統一且不洩漏內部資訊；非擁有者存取對話一律 404；CSRF 用 Origin 比對加 `SameSite=Lax`；SSE 用資料庫 `generating_since` 原子 claim 防止雙串流；PATCH 採 merge-patch；管理者遮蔽（`message`）在輸出層集中處理。

**已處理**：送出訊息即串流回覆（`POST …/messages` 直接回 SSE，`GET …/stream` 只作重試）；文件與實作漂移已修正；API 契約改由 utoipa 產生（`GET /api/openapi.json`）。

**待處理**（各自已開 issue，改端點時要同步改 OpenAPI 註解與前端呼叫點）：

### 沒有分頁／上限【高】（#284 對話、#285 總結、#286 學生、#287 名單、#288 活動）

`GET /api/conversations`、`/api/teacher/summaries`、`/api/classrooms/{id}/students`、`/api/classrooms/{id}/roster`、`/api/activities` 除 #284 對話已完成 keyset 分頁外，其餘仍一次 `fetch_all`。`teacher/summaries` 還 JOIN 三表且隨全班歷史成長，會是第一個變慢的端點。
**建議**：統一 `?limit=&cursor=`（keyset，依 `ended_at, id`），回 `{ items, next_cursor }`。教師端再加 `?student=`、`?topic=` 篩選。因為還沒上線，現在改成本最低。

### 動作式路徑與動詞誤用【中】（#290 帳號啟用狀態、#291 可選清單）

- #290 已完成：`PUT /api/admin/users/{id}/status` 設定啟用狀態；`end`／`publish`／`close` 這類狀態轉換用動作式 POST 是常見務實做法，可保留，但同一組資源請擇一風格。
- 資源識別用 email 放在 path：停用／復原帳號的對象一定有帳號，改用 user id（UUID，#290）；名單與教師項目可能沒有帳號，維持 email。目前後端沒有掛 request logging（無 `TraceLayer`），路徑不會進日誌；日後加上時要過濾路徑中的 email。
- #291 已完成：學生可選清單改為 `GET /api/catalog`。

### 同一資源兩組路徑【中】（#292、#293、#294）

#292–#294 已完成：題庫只保留 `/api/topics`，權限由角色決定；admin 前綴留給只有管理者能使用的功能。

### 角色與資源的混合命名【中】（#285 `/api/summaries`）

`/api/teacher/summaries`、`/api/dashboard/class`、`/api/classrooms/{id}/roster`、`/api/classrooms/{id}/students` 混用「以角色命名」與「以資源命名」。學生的 summary 是 `/conversations/{id}/summary`，教師的是 `/teacher/summaries`。
**建議**：以資源命名，權限由後端判斷，例如 `GET /api/summaries`（教師全部／學生自己的）；或維持角色前綴但全面一致（`/api/teacher/*`、`/api/admin/*`、`/api/student/*`）。目前兩者混用，前端與文件都要記例外。

### `must_change_password` 白名單的回歸測試【中】

臨時密碼旗標由 session 層擋掉其他 API（403 `password_change_required`），這設計合理。風險是新增路由時若忘了走同一個 extractor 就會漏擋；`auth.rs` 若是在 extractor 內統一檢查，建議補一個「遍歷所有路由、確認在旗標下皆為 403」的整合測試，以後新增 F-17/F-19 端點時自動保護。

### 其他細節【低】

- `POST /api/conversations/{id}/end` 回 202（首次）與 200（已結束）：狀態碼雙義，對前端是額外分支。建議一律 202 或一律 200 + body `{ status }`。
- `POST /api/conversations` 的來源錯誤回 `invalid_conversation_source`；`POST …/messages` 的非法 `source` 仍回 `invalid_source`。
- `POST /api/classrooms/{id}/roster/import` 一次回傳明文臨時密碼：已限制只出現一次、僅管理者，OK；但回應建議加 `Cache-Control: no-store`。
- 登入鎖定 5 次／15 分鐘是以信箱為單位，可被用來鎖住他人帳號；加 IP 維度見 #295（會改 S-08.2，需先更新規格）。
- 沒看到全域 request body 大小限制與請求逾時（`tower_http` 只用在靜態檔）。語音有 10 MB 上限，其餘靠 axum 預設 2 MB；建議明確設定並加 `TimeoutLayer`（SSE 路由除外）。
- `/health` 之外沒有 `/ready`（見「規劃中的端點」）。
- 稽核（`audit_logs`）涵蓋登入、disable／enable／delete、教師與名單異動、reset-password（F-22.1）；日後的調價（S-07 §5 要求）也要進 `audit_logs`。

## 建議的處理順序

1. 分頁與上限、`/ready`、body 限制：趁未上線一次做。
2. 路徑整理（動作式路徑、重複路徑、命名）：先定命名規則寫進 `docs/specs/`，再批次改。
3. 新功能（上表）依 S-07 → F-19.1 → F-19.2/19.3 → F-17 的相依順序，沿用「事件內入帳 + UNIQUE 冪等」設計，不開放前端寫入點數。

## 本機執行

```bash
docker compose up -d db
cp .env.example .env   # 專案根目錄，整個系統共用；填入 GOOGLE_CLIENT_ID／SECRET、OPENAI_MODEL、ADMIN_EMAILS
cp .apikeylist.example .apikeylist   # 一行一把 AI API key；後端從 ./.apikeylist 或 ../.apikeylist 讀取
set -a && . ./.env && set +a   # 後端不會自行讀取 .env，需先載入環境變數
cd backend
cargo run              # RUN_MIGRATIONS=true 時啟動前自動 migration
```

啟動後可從 `http://127.0.0.1:3000/api/openapi.json` 取得 OpenAPI 文件。

要連前端一起跑：在 `frontend/` 執行 `npm ci && npm run build`，並設定 `FRONTEND_DIR=../frontend/dist`，後端就會在同一個網域提供前端（找不到的路徑退回 `index.html`，`/api/*` 仍回 JSON 404）。開發前端時改用 `npm run dev`（Vite 會把 `/api` 轉給本機後端）。

具體 OpenAI 模型尚未決定（S-03.1／S-03.4），因此 `OPENAI_MODEL` 沒有預設值。
