# Workers 試行版（Cloudflare Workers Free ＋ D1）

> **這是試行，不是規格。** `AGENTS.md` 規定的技術是 Rust＋Axum＋SQLx＋PostgreSQL；這個目錄只用來評估「整個後端改跑在 Workers 免費方案、資料庫改 D1（SQLite）」是否可行。要採用必須先走規格決策（`docs/specs/decisions.md`）。部署只放合成資料，與 NTNU 正式環境完全無關。

## 內容

| 路徑 | 說明 |
| --- | --- |
| `src/` | 由 `backend/src` 複製後改寫：Axum router 不變，執行環境換成 `workers-rs`（`#[event(fetch)]`、`#[event(scheduled)]`） |
| `sqlx-d1/` | 小型 shim，crate 名稱是 `sqlx`：提供後端用到的 `query`／`query_as`／`query_scalar`、`PgPool`、`Transaction` 介面，底層呼叫 D1 |
| `migrations/0001_init.sql` | 14 筆 PostgreSQL migration 的最終 schema 譯成 SQLite |
| `scripts/check_sql.py` | 抽出所有 SQL，套用 shim 的改寫後對 schema 跑 `EXPLAIN`（`python3 -I scripts/check_sql.py src migrations`） |
| `wrangler.toml` | Worker、D1 binding、靜態檔（`frontend/dist`）、cron |

## 和 PostgreSQL 版的差異

**執行環境**

- `tokio` 只留 `sync`／`macros`／`rt`。`rt` 模組以 JS 時鐘實作 `Instant`、`sleep`、`timeout`；`std::time::Instant` 在 wasm32 會 panic，不可使用。
- 回應送出後的背景工作（AI 回覆寫入、總結生成）用 `ctx.waitUntil`：瀏覽器斷線後最多再跑 30 秒。
- 卡住的總結由 cron（每 10 分鐘）執行 `recover_orphaned`，取代 PostgreSQL 版啟動時與背景迴圈的回收。
- `reqwest` 換成以 `fetch` 實作的 `http` 模組（同樣的 builder 介面）。
- Google 登入自行實作 OIDC（PKCE、state、nonce）。ID token 直接從 token endpoint 經 TLS 取得，依 OIDC Core 3.1.3.7 只驗證 claims，不驗簽章。
- 用戶端 IP 取自 `CF-Connecting-IP`。

**資料庫（D1）**

- `$N` 改寫成 `?N`，`now()` 改寫成 ISO-8601 毫秒字串；UUID、時間、JSON 陣列都存成 TEXT，布林值存成 0/1。
- **沒有互動式交易**：`begin()`／`commit()` 是空操作，每個語句各自提交。必須一起成立的寫入改用 `Pool::batch`（D1 batch 是原子的），例如建立活動與題目、AI 回覆與階段更新。可能失敗的檢查一律移到寫入之前（成就門檻、成就識別碼、未回覆訊息的檢查）。
- **沒有列鎖**（`FOR UPDATE`／`FOR SHARE`／advisory lock 已移除）。唯一索引仍然擋住重複資料；已知的競態：兩位管理者同時停用彼此，可能停用掉最後一位管理者。
- SQLite 對欄位上的唯一索引只回報欄位清單，啟動時以 `sqlx::register_constraint_aliases` 對應回 PostgreSQL 的約束名稱。
- 台北時間的每日／每週任務區間改在 Rust 計算（UTC+8，無夏令時間）。

**功能**

- 移除 cascade 即時語音（WebSocket）；語音的 STT／TTS HTTP 端點保留。
- 密碼雜湊由 Argon2 改成 PBKDF2-SHA256（WebCrypto），格式 `$pbkdf2-sha256$i=N$salt$hash`：使用者密碼 100,000 輪，臨時密碼與示範密碼 1,000 輪。**舊的 Argon2 雜湊無法轉換**，從 PostgreSQL 搬資料時要由管理者重設密碼。
- 下列記憶體狀態是「每個 isolate 各自一份」，多個 isolate 之間不共享：登入限流、AI 用量上限、翻譯頻率限制、API key 冷卻、管理介面的請求日誌、重新接上進行中的回覆。

## 部署

需要 `worker-build`（`cargo install worker-build`）與 Node（`npx wrangler`）。憑證只用環境變數傳入，不寫進檔案：

```bash
export CLOUDFLARE_ACCOUNT_ID=...   CLOUDFLARE_API_TOKEN=...
cd worker
npx wrangler d1 create socrates-chat-trial          # 把輸出的 database_id 填入 wrangler.toml
npx wrangler d1 migrations apply DB --remote
(cd ../frontend && npm ci && npm run build)          # 產生 ../frontend/dist
npx wrangler secret put OPENAI_API_KEYS              # 另有 GOOGLE_CLIENT_SECRET、ADMIN_EMAILS、ADMIN_INITIAL_PASSWORD
npx wrangler deploy --var APP_BASE_URL:https://<worker>.<subdomain>.workers.dev
```

本機測試：`.dev.vars` 放設定（已列入 `.gitignore`），`npx wrangler d1 migrations apply DB --local` 後執行 `npx wrangler dev`。cron 用 `curl "http://localhost:8787/cdn-cgi/handler/scheduled"` 觸發。

## 試行結果（2026-11-10）

- wasm 5.2 MB，gzip 後 1.46 MB（免費方案上限 3 MB）；啟動時間 4 ms。
- `cargo clippy --target wasm32-unknown-unknown -- -D warnings` 通過；`check_sql.py` 檢查 170 條靜態 SQL 全數通過，另有 8 條動態 SQL 已人工確認。
- 端對端煙霧測試（登入、教室、名單匯入、活動、開始對話、重複開始回 409、SSE 回覆、結束、背景總結、儀表板、點數、任務、管理介面、權限）：本機以假 AI 端點 **42/42** 通過；部署到 workers.dev 後 **40/43**，失敗的 3 項都需要真的 AI key（部署只放了佔位 key，AI 失敗時正確送出 `error` 事件，總結標成 `failed`）。
- CPU 時間（`wrangler tail`，29 個請求，全部 `ok`、沒有超時中止）：中位數 7 ms；冷啟動 56 ms；**登入 7–27 ms**，常超過免費方案的 10 ms。Cloudflare 容許偶爾超出，持續超出會回 1102 錯誤，是免費方案最大的風險。
- 未驗證：真實 AI 串流的長回覆在 `waitUntil` 30 秒內能否寫完、Google 登入（需要 OAuth client）、語音 STT／TTS、多 isolate 下的限流行為。
