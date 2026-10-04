# MVP 後端 API 參考

> 這份文件描述**目前已實作**的後端 API，供前端依循；它不是規格，規則以 [`docs/specs/`](../specs/decisions.md) 為準。
> 所有路徑在同一網域的 `/api` 之下（S-08.1）。認證靠 `sid` HttpOnly cookie。

## 通則

- **CSRF**：`POST`／`PUT`／`PATCH`／`DELETE` 必須帶 `Origin` 標頭，且等於 `APP_BASE_URL`，否則 `403 csrf`（瀏覽器會自動帶）。
- **錯誤格式**：`{ "error": { "code": "...", "request_id": "..." } }`；只有 `code`，沒有已翻譯文字，前端依 `code` 顯示（S-12.1）。回應標頭也有 `x-request-id`。
- **常見錯誤碼**：`unauthorized`（401）、`forbidden`（403）、`account_disabled`（403，帳號已被管理者停用）、`password_change_required`（403，用臨時密碼登入尚未改密碼）、`not_enrolled`（403，名單外帳號）、`not_found`（404）、`csrf`（403）、`internal`（500）。
- **PATCH 語意**：沒帶的欄位保留原值；可為空的欄位（題目的 `category`、活動的 `topic_id`）送 `null` 代表清除（JSON merge-patch，RFC 7396）。`description` 不可為 `null`，要清空請送 `""`。
- 對話內容只有擁有者讀得到；別人（含教師、管理者）一律 `404`。

## 登入

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/auth/google/login` | 導向 Google 授權頁（303） |
| GET | `/api/auth/google/callback` | Google 回呼；成功設定 `sid` cookie 並導回 `APP_BASE_URL`，失敗導回 `/login?error=<code>`（`invalid_state`、`email_not_verified`、`access_denied`、`login_failed`） |
| POST | `/api/auth/login` | 內建登入，body `{ email, password }` → `{ must_change_password }` 並設定 `sid` cookie；失敗 401 `invalid_credentials`（帳號不存在與密碼錯誤相同）；同一信箱連續失敗 5 次鎖定 15 分鐘，期間回 429 `too_many_attempts` |
| POST | `/api/auth/change-password` | body `{ current_password, new_password }` → 204；`invalid_current_password`、`invalid_new_password`（長度 8–128）、`password_unchanged`（皆 400）；成功後其他 session 失效 |
| POST | `/api/auth/logout` | 登出，204 |
| GET | `/api/me` | `{ id, email, display_name, is_admin, must_change_password, roles: { admin, teacher, student } }`；`must_change_password` 為 `true` 時，前端應導向改密碼頁，其他 API 會回 `password_change_required` |

沒有註冊 API：內建帳號只由系統建立（管理者重設密碼、匯入修課名單、部署時的 `ADMIN_INITIAL_PASSWORD`）。

`roles` 全為 `false` 代表「尚未開通」。教師與管理者不需要在修課名單內。

## 管理（管理者）

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET／POST | `/api/admin/teachers` | 列出／新增教師，body `{ "email": "..." }`（204，重複新增也 204；`invalid_email` 400） |
| DELETE | `/api/admin/teachers/{email}` | 移除教師身分（204／404） |
| POST | `/api/admin/users/reset-password` | body `{ "email": "..." }` → `{ email, temporary_password }`（臨時密碼只出現這一次）；沒有帳號就建立；清除該使用者所有 session 與登入鎖定 |
| GET／POST | `/api/admin/topics` | 列出全部／新增題目 `{ title, description?, category? }`（201） |
| PATCH | `/api/admin/topics/{id}` | 修改 `{ title?, description?, category?, is_active? }`；停用後學生看不到 |
| POST | `/api/admin/users/{email}/disable` | 停用帳號，body `{ reason? }`（204）；立即清除該帳號所有 session，資料保留；`cannot_disable_self`、`last_admin`（409）；寫入稽核日誌 |
| POST | `/api/admin/users/{email}/enable` | 復原帳號，body `{ reason? }`（204）；寫入稽核日誌 |

## 題庫（教師或管理者）

教師也能放入題目（S-01.3）。介面與管理者的 `/api/admin/topics` 相同：

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET／POST | `/api/topics` | 列出全部／新增題目 |
| PATCH | `/api/topics/{id}` | 修改或啟用／停用 |

系統啟動時會補上內建題目「電車難題」（固定 ID；已存在就不動，教師改過或停用後也不會被覆寫）。

## 修課名單（教師或管理者）

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/roster` | `{ emails: [...] }` |
| POST | `/api/roster/import` | body `{ "text": "每行一個電子郵件（可有 email 標題列）" }` → `{ added, existing, invalid: [{ line, value }], credentials: [{ email, temporary_password }] }`；只新增不移除；`credentials` 是這次新建立的內建帳號與臨時密碼（只出現這一次；**只有管理者匯入才會建立**，教師匯入時為空陣列；已有帳號與教師／管理者信箱不在其中） |
| DELETE | `/api/roster/{email}` | 移出名單（204／404）；資料保留 |
| GET | `/api/students` | 名單上的學生與帳號狀態：`[{ email, display_name, has_account, disabled, completed_conversations }]`（只有完成對話的數量，沒有內容） |
| POST | `/api/students/{email}/delete` | 刪除學生帳號（教師或管理者）：body `{ confirm_email }` 必須與路徑的電子郵件相同（`confirmation_mismatch` 400）；一併刪除對話、總結與名單項目；教師與管理者帳號不能由此刪除（`not_a_student` 409）；寫入稽核日誌（204） |

## 活動（教師或管理者）與學生可選清單

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET／POST | `/api/activities` | 列表／建立 `{ title, description?, topic_id? }`（201，狀態 `draft`） |
| GET／PATCH | `/api/activities/{id}` | 讀取／修改 `{ title?, description?, topic_id? }` |
| POST | `/api/activities/{id}/publish` | 發布（`status: published`），可重新開放 |
| POST | `/api/activities/{id}/close` | 停止開放（`status: closed`） |
| GET | `/api/available` | 學生可選：`{ activities: [已發布活動], topics: [已啟用題目] }` |

## 對話（只有修課名單內的學生）

教師與管理者不參與討論：下列對話、可選清單、語音與 `/api/dashboard/me` 對教師與管理者一律回 `403 forbidden`（名單外帳號回 `not_enrolled`）。

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| POST | `/api/conversations` | `{ activity_id }` 或 `{ topic_id }`（201）；來源須為已發布活動／已啟用題目，否則 `400 invalid_source` |
| GET | `/api/conversations` | 我的對話列表（不含訊息） |
| GET | `/api/conversations/{id}` | 對話與全部訊息；含 `status`（`active`／`ended`）、`stage`（1–3）、`turn_count`、`converge_ready` |
| DELETE | `/api/conversations/{id}` | 刪除我的對話（含訊息與總結），204 |
| POST | `/api/conversations/{id}/messages` | `{ content, source? }`（`text`／`speech-browser`／`speech-openai`）→ 201 訊息，另有 `auto_ended`（達回合上限，對話已自動結束、總結產生中） |
| GET | `/api/conversations/{id}/stream` | **SSE**：送出學生訊息後開啟以取得 AI 回覆。事件：`delta` `{text}`、`done` `{message_id, question_type, stage, suggest_end}`、`error` `{code}`（`ai_unavailable`＝顯示「AI 暫時無法回覆」與重試按鈕；重試就是再開一次串流） |
| POST | `/api/conversations/{id}/end` | 結束對話並在背景產生總結（202；已結束回 200；`empty_conversation` 409；AI 還在回覆時 `reply_in_progress` 409） |
| GET | `/api/conversations/{id}/summary` | `{ status: pending／ready／failed, stance, reasons, turning_points, position, framework, completed_at }`；`claim` 是學生最終主張的一句短句（AI 寫的，約 30 字內，用對話語言，分組用）；`framework`（主要學派，6 個倫理學派或 `other`）是 AI 標記的輔助標籤，`framework_scores`（`{ utilitarianism, deontology, virtue, contractarianism, care, existentialism }`，各為 0–5 的整數）是 AI 替這份論點總結在 6 個學派維度上打的分數，都是暫定；沒有時為 `null`（分數必須 6 項齊全才採用）；`pending` 時請輪詢 |
| POST | `/api/conversations/{id}/summary/retry` | 總結失敗後重試（202；`summary_not_retryable` 409） |

對話錯誤碼：`conversation_ended`（409）、`reply_in_progress`（409，AI 回覆中不接受新訊息或第二條串流）、`nothing_to_reply`（409）、`invalid_message`（400，空白或超過 4000 字元）。

## 儀表板

論點分布的主體是「主張」：AI 為每場對話寫一句最終主張，同一題目下再把意思相近的歸成一群並命名（暫定，S-04 尚未決定；轉變先不記，只有最終主張）。`frameworks` 是各主要學派的人數（含 `unclassified`，值可為 0），只是輔助資訊。`radar` 是 6 維雷達圖的平均：`{ scored, max: 5, average: { 各學派: 0–5 平均（小數 2 位） } }`，只平均有 6 維分數的總結，`scored` 是這類總結的數量（沒有時各維度為 0）。

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/dashboard/me` | 個人：`{ total_conversations, total_turns, completed, frameworks, radar, recent: [最近完成的 10 場，含 framework_scores] }`，只算自己的資料 |
| GET | `/api/dashboard/class` | 班上論點分布（只給教師，S-02.4 不設最小群體門檻）：`{ masked, students_total, students_participating, completed, frameworks, radar, topics: [{ title, completed, frameworks, radar, claims, ungrouped }] }`；只有數量與分類，沒有學生身分或內容。管理者身分一律 `masked: true` 且沒有分布（S-02.2）。`claims` 是該題目的主張分布：`[{ name, other, count, reasons, radar }]`，人數多的在前；`reasons` 是這群人的核心理由（AI 總結的文字，最多 5 則、去重，沒有姓名與原文）；超過 6 群時最小的幾群併成 `other: true`（沒有名稱）；`radar` 是該群的 6 維平均。`ungrouped` 是有主張但還沒歸群的場數（分組失敗或尚未分組） |
| POST | `/api/dashboard/class/regroup` | body `{ title }`（題目標題）：教師請 AI 把該題目下全部主張重新分組（204）；沒有可分組的主張回 `409 no_claims`，AI 失敗回 `502 ai_unavailable` 並沿用原本的分組；管理者 403 |

## 語音備援（只有修課名單內的學生）

瀏覽器預設用 Web Speech API；不支援時才經這裡呼叫 OpenAI（S-05.3）。**不保存原始音訊**（S-05.4）。具體語音模型尚未決定，沒設定 `OPENAI_STT_MODEL`／`OPENAI_TTS_MODEL` 時回 `503 voice_unavailable`；外部服務失敗回 `502 voice_failed`。

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| POST | `/api/voice/transcribe?language=zh-TW\|en\|es` | 請求本文是音訊位元組（`Content-Type: audio/*`，上限 10 MB）→ `{ text }` |
| POST | `/api/voice/speech` | body `{ text, language }`（文字 1–4000 字元）→ `audio/mpeg` |

訊息的 `source` 為 `speech-browser`／`speech-openai` 時代表該則訊息是語音輸入（逐字稿，S-05.4）。

## 教師

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/teacher/summaries` | 所有修課學生已完成對話的總結：`[{ conversation_id, title, ended_at, student_id, student_name, student_email, stance, reasons, turning_points, position, framework }]`；**不含對話原文**。管理者身分一律看到固定字串 `message` |

## 本機執行

```bash
docker compose up -d db
cp .env.example .env   # 專案根目錄，整個系統共用；填入 GOOGLE_CLIENT_ID／SECRET、OPENAI_API_KEY、OPENAI_MODEL、ADMIN_EMAILS
set -a && . ./.env && set +a   # 後端不會自行讀取 .env，需先載入環境變數
cd backend
cargo run              # RUN_MIGRATIONS=true 時啟動前自動 migration
```

要連前端一起跑：在 `frontend/` 執行 `npm ci && npm run build`，並設定 `FRONTEND_DIR=../frontend/dist`，後端就會在同一個網域提供前端（找不到的路徑退回 `index.html`，`/api/*` 仍回 JSON 404）。開發前端時改用 `npm run dev`（Vite 會把 `/api` 轉給本機後端）。

具體 OpenAI 模型尚未決定（S-03.1／S-03.4），因此 `OPENAI_MODEL` 沒有預設值。
