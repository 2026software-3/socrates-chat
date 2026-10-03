# MVP 後端 API 參考

> 這份文件描述**目前已實作**的後端 API，供前端依循；它不是規格，規則以 [`docs/specs/`](../specs/decisions.md) 為準。
> 所有路徑在同一網域的 `/api` 之下（S-08.1）。認證靠 `sid` HttpOnly cookie。

## 通則

- **CSRF**：`POST`／`PUT`／`PATCH`／`DELETE` 必須帶 `Origin` 標頭，且等於 `APP_BASE_URL`，否則 `403 csrf`（瀏覽器會自動帶）。
- **錯誤格式**：`{ "error": { "code": "...", "request_id": "..." } }`；只有 `code`，沒有已翻譯文字，前端依 `code` 顯示（S-12.1）。回應標頭也有 `x-request-id`。
- **常見錯誤碼**：`unauthorized`（401）、`forbidden`（403）、`not_enrolled`（403，名單外帳號）、`not_found`（404）、`csrf`（403）、`internal`（500）。
- **PATCH 語意**：沒帶的欄位保留原值；可為空的欄位（題目的 `category`、活動的 `topic_id`）送 `null` 代表清除（JSON merge-patch，RFC 7396）。`description` 不可為 `null`，要清空請送 `""`。
- 對話內容只有擁有者讀得到；別人（含教師、管理者）一律 `404`。

## 登入

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/auth/google/login` | 導向 Google 授權頁（303） |
| GET | `/api/auth/google/callback` | Google 回呼；成功設定 `sid` cookie 並導回 `APP_BASE_URL`，失敗導回 `/login?error=<code>`（`invalid_state`、`email_not_verified`、`access_denied`、`login_failed`） |
| POST | `/api/auth/logout` | 登出，204 |
| GET | `/api/me` | `{ id, email, display_name, is_admin, roles: { admin, teacher, student } }` |

`roles` 全為 `false` 代表「尚未開通」。教師與管理者不需要在修課名單內。

## 管理（管理者）

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET／POST | `/api/admin/teachers` | 列出／新增教師，body `{ "email": "..." }`（204，重複新增也 204；`invalid_email` 400） |
| DELETE | `/api/admin/teachers/{email}` | 移除教師身分（204／404） |
| GET／POST | `/api/admin/topics` | 列出全部／新增題目 `{ title, description?, category? }`（201） |
| PATCH | `/api/admin/topics/{id}` | 修改 `{ title?, description?, category?, is_active? }`；停用後學生看不到 |

## 修課名單（教師或管理者）

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/roster` | `{ emails: [...] }` |
| POST | `/api/roster/import` | body `{ "text": "每行一個電子郵件（可有 email 標題列）" }` → `{ added, existing, invalid: [{ line, value }] }`；只新增不移除 |
| DELETE | `/api/roster/{email}` | 移出名單（204／404）；資料保留 |

## 活動（教師或管理者）與學生可選清單

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET／POST | `/api/activities` | 列表／建立 `{ title, description?, topic_id? }`（201，狀態 `draft`） |
| GET／PATCH | `/api/activities/{id}` | 讀取／修改 `{ title?, description?, topic_id? }` |
| POST | `/api/activities/{id}/publish` | 發布（`status: published`），可重新開放 |
| POST | `/api/activities/{id}/close` | 停止開放（`status: closed`） |
| GET | `/api/available` | 學生可選：`{ activities: [已發布活動], topics: [已啟用題目] }` |

## 對話（學生；教師與管理者也可用自己的對話）

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| POST | `/api/conversations` | `{ activity_id }` 或 `{ topic_id }`（201）；來源須為已發布活動／已啟用題目，否則 `400 invalid_source` |
| GET | `/api/conversations` | 我的對話列表（不含訊息） |
| GET | `/api/conversations/{id}` | 對話與全部訊息；含 `status`（`active`／`ended`）、`stage`（1–3）、`turn_count`、`converge_ready` |
| DELETE | `/api/conversations/{id}` | 刪除我的對話（含訊息與總結），204 |
| POST | `/api/conversations/{id}/messages` | `{ content, source? }`（`text`／`speech-browser`／`speech-openai`）→ 201 訊息，另有 `auto_ended`（達回合上限，對話已自動結束、總結產生中） |
| GET | `/api/conversations/{id}/stream` | **SSE**：送出學生訊息後開啟以取得 AI 回覆。事件：`delta` `{text}`、`done` `{message_id, question_type, stage, suggest_end}`、`error` `{code}`（`ai_unavailable`＝顯示「AI 暫時無法回覆」與重試按鈕；重試就是再開一次串流） |
| POST | `/api/conversations/{id}/end` | 結束對話並在背景產生總結（202；已結束回 200；`empty_conversation` 409；AI 還在回覆時 `reply_in_progress` 409） |
| GET | `/api/conversations/{id}/summary` | `{ status: pending／ready／failed, stance, reasons, turning_points, completed_at }`；`pending` 時請輪詢 |
| POST | `/api/conversations/{id}/summary/retry` | 總結失敗後重試（202；`summary_not_retryable` 409） |

對話錯誤碼：`conversation_ended`（409）、`reply_in_progress`（409，AI 回覆中不接受新訊息或第二條串流）、`nothing_to_reply`（409）、`invalid_message`（400，空白或超過 4000 字元）。

## 教師

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/teacher/summaries` | 所有修課學生已完成對話的總結：`[{ conversation_id, title, ended_at, student_id, student_name, student_email, stance, reasons, turning_points }]`；**不含對話原文**。管理者身分一律看到固定字串 `message` |

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
