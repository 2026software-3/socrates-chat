# 後端 API 說明

> 端點清單、路徑、參數與請求／回應欄位**不寫在這裡**，以 OpenAPI 3 為準：後端執行時公開在 `GET /api/openapi.json`，由各 handler 的 `#[utoipa::path]` 註解與型別的 `ToSchema` 產生（新增或修改端點時要同步改註解；`backend/tests/openapi.rs` 檢查已文件化的端點都有路由且受保護）。
> 這份文件只補 OpenAPI 寫不下的**規則與流程**。它不是規格，規則以 [`docs/specs/`](../specs/decisions.md) 為準。
> 所有路徑在同一網域的 `/api` 之下（S-08.1）。認證靠 `sid` HttpOnly cookie。

## 通則

- **CSRF**：`POST`／`PUT`／`PATCH`／`DELETE` 必須帶 `Origin` 標頭，且等於 `APP_BASE_URL`，否則 `403 csrf`（瀏覽器會自動帶）。
- **錯誤格式**：`{ "error": { "code": "...", "request_id": "..." } }`；只有 `code`，沒有已翻譯文字，前端依 `code` 顯示（S-12.1）。回應標頭也有 `x-request-id`。
- **常見錯誤碼**：`unauthorized`（401）、`forbidden`（403）、`account_disabled`（403，帳號已被管理者停用）、`password_change_required`（403，用臨時密碼登入尚未改密碼）、`not_enrolled`（403，名單外帳號）、`not_found`（404）、`csrf`（403）、`internal`（500）。各端點特有的錯誤碼寫在 OpenAPI 的 responses 說明。
- **PATCH 語意**：沒帶的欄位保留原值；可為空的欄位（題目的 `category`、活動的 `topic_id`）送 `null` 代表清除（JSON merge-patch，RFC 7396）。`description` 不可為 `null`，要清空請送 `""`。
- **權限分層**：管理者專用的在 `/api/admin`；教師或管理者；只有修課名單內的學生。對話內容只有擁有者讀得到，別人（含教師、管理者）一律 `404`。

## 登入

- 沒有註冊 API：內建帳號只由系統建立（管理者重設密碼、管理者匯入修課名單、部署時的 `ADMIN_INITIAL_PASSWORD`）。
- 內建登入失敗一律回同一個 `invalid_credentials`（帳號不存在與密碼錯誤相同）；同一信箱連續失敗 5 次鎖定 15 分鐘，期間回 `429 too_many_attempts`。
- Google 回呼成功設定 `sid` cookie 並導回 `APP_BASE_URL`；失敗導回 `/login?error=<code>`（`invalid_state`、`email_not_verified`、`access_denied`、`login_failed`）。
- 用臨時密碼登入時 `must_change_password` 為 `true`：前端導向改密碼頁，除了 `GET /api/me`、改密碼與登出，其他 API 都回 `password_change_required`。旗標在 session 上，不在使用者上。
- 改密碼成功後，該使用者**其他** session 失效（保留目前這個）。新密碼長度 8–128。
- `GET /api/me` 的 `roles` 全為 `false` 代表「尚未開通」。教師與管理者不需要在修課名單內。

## 管理（管理者）

- 重設密碼：臨時密碼**只出現在這次回應**；沒有帳號就建立；同時清除該使用者所有 session 與登入鎖定。
- 停用帳號：立即清除該帳號所有 session，資料保留；不能停用自己（`cannot_disable_self`），系統至少保留 1 位啟用中的管理者（`last_admin`）；停用與復原都寫入稽核日誌。
- 新增教師重複送出也回 204（冪等）。

## 題庫與活動

- 教師也能放入題目（S-01.3）。`/api/admin/topics` 與 `/api/topics` 目前是同功能的兩組路徑（合併見 issue #292–#294）。
- 題目停用後學生看不到。系統啟動時會補上內建題目「電車難題」（固定 ID；已存在就不動，教師改過或停用後也不會被覆寫）。
- 活動建立時是 `draft`；`publish` 後學生才選得到，可重新開放；`close` 停止開放。
- 學生可選清單只含已發布的活動與已啟用的題目。

## 修課名單與學生

- 匯入只新增、不移除；每行一個電子郵件，可有 `email` 標題列。
- 匯入結果的 `credentials` 是這次新建立的內建帳號與臨時密碼（只出現這一次）。**只有管理者匯入才會建立帳號**，教師匯入時為空陣列；已有帳號與教師／管理者的信箱不在其中。
- 移出名單不刪資料（重新加回後照舊）。
- 刪除學生帳號：`confirm_email` 必須與路徑的電子郵件相同（`confirmation_mismatch`）；一併刪除對話、總結與名單項目；教師與管理者帳號不能由此刪除（`not_a_student`）；寫入稽核日誌。
- 學生列表只有完成對話的**數量**，沒有內容。

## 對話（只有修課名單內的學生）

教師與管理者不參與討論：對話、可選清單、語音與 `/api/dashboard/me` 對教師與管理者一律回 `403 forbidden`（名單外帳號回 `not_enrolled`）。

- 開始對話的來源須為已發布活動或已啟用題目，否則 `400 invalid_source`。
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
  - `framework`：主要學派（6 個倫理學派或 `other`），AI 標記的輔助標籤。
  - `framework_scores`：6 個學派維度各 0–5 的整數；必須 6 項齊全才採用，沒有時為 `null`。以上皆為暫定。
- 刪除對話會一併刪除訊息與總結。

## 儀表板

論點分布的主體是「主張」：AI 為每場對話寫一句最終主張，同一題目下再把意思相近的歸成一群並命名（轉變先不記，只有最終主張）。

- `frameworks` 是各主要學派的人數（含 `unclassified`，值可為 0），只是輔助資訊。
- `radar`：6 維雷達圖的平均，`{ scored, max: 5, average }`；只平均有 6 維分數的總結，`scored` 是這類總結的數量（沒有時各維度為 0）。
- 個人儀表板只算自己的資料。
- 班上儀表板只給教師（S-02.4 不設最小群體門檻），只有數量與分類，沒有學生身分或內容。管理者身分一律 `masked: true` 且沒有分布（S-02.2）。
- 題目的 `claims`：人數多的在前；`reasons` 是這群人的核心理由（AI 總結的文字，最多 5 則、去重，沒有姓名與原文）；超過 6 群時最小的幾群併成 `other: true`（沒有名稱）；`ungrouped` 是有主張但還沒歸群的場數（分組失敗或尚未分組）。
- 重新分組：教師請 AI 把該題目下全部主張重新分組。沒有可分組的主張回 `409 no_claims`；AI 失敗回 `502 ai_unavailable` 並沿用原本的分組；管理者 `403`。

## 語音備援（只有修課名單內的學生）

瀏覽器預設用 Web Speech API；不支援時才經 `/api/voice/*` 呼叫 OpenAI（S-05.3）。**不保存原始音訊**（S-05.4）。具體語音模型尚未決定，沒設定 `OPENAI_STT_MODEL`／`OPENAI_TTS_MODEL` 時回 `503 voice_unavailable`；外部服務失敗回 `502 voice_failed`。語音辨識的請求本文是音訊位元組（`Content-Type: audio/*`，上限 10 MB），`language` 為 `zh-TW`、`en` 或 `es`；合成文字 1–4000 字元。

## 教師檢視

教師檢視學生的總結**不含對話原文**（S-02.1）。管理者身分的敏感欄位一律看到固定字串 `message`（S-02.2）。

## 本機執行

```bash
docker compose up -d db
cp .env.example .env   # 專案根目錄，整個系統共用；填入 GOOGLE_CLIENT_ID／SECRET、OPENAI_API_KEY、OPENAI_MODEL、ADMIN_EMAILS
set -a && . ./.env && set +a   # 後端不會自行讀取 .env，需先載入環境變數
cd backend
cargo run              # RUN_MIGRATIONS=true 時啟動前自動 migration
```

啟動後可從 `http://127.0.0.1:3000/api/openapi.json` 取得 OpenAPI 文件。

要連前端一起跑：在 `frontend/` 執行 `npm ci && npm run build`，並設定 `FRONTEND_DIR=../frontend/dist`，後端就會在同一個網域提供前端（找不到的路徑退回 `index.html`，`/api/*` 仍回 JSON 404）。開發前端時改用 `npm run dev`（Vite 會把 `/api` 轉給本機後端）。

具體 OpenAI 模型尚未決定（S-03.1／S-03.4），因此 `OPENAI_MODEL` 沒有預設值。
