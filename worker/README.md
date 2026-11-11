# Workers 試行版（Cloudflare Workers Free ＋ Hyperdrive ＋ Supabase PostgreSQL）

> **這是試行，不是規格。** `AGENTS.md` 規定的技術是 Rust＋Axum＋SQLx＋PostgreSQL，並且**不引入 Supabase**；這個分支是需求方同意的試行例外，只用來評估「後端改跑在 Workers 免費方案、資料庫用 Supabase 託管的 PostgreSQL」是否可行。要採用必須先走規格決策（`docs/specs/decisions.md`），`docs/specs/` 沒有任何修改。部署只放合成資料，與 NTNU 正式環境完全無關。
>
> D1（SQLite）版在 `feat/workers-d1` 分支，兩版並存以便對照。

## 內容

| 路徑 | 說明 |
| --- | --- |
| `src/` | 由 `backend/src` 複製後改寫：Axum router 不變，執行環境換成 `workers-rs`（`#[event(fetch)]`、`#[event(scheduled)]`）；SQL 與 `backend/src` 相同 |
| `sqlx-pg/` | 小型 shim，crate 名稱是 `sqlx`：提供後端用到的 `query`／`query_as`／`query_scalar`、`PgPool`、`Transaction` 介面，底層是 tokio-postgres 經 Workers socket 連到 Hyperdrive |
| `scripts/check_sql.py` | 抽出所有靜態 SQL 輸出成 JSON，讓呼叫端對套用 `backend/migrations` 的資料庫逐條 `PREPARE`（只解析、不執行） |
| `scripts/supabase_lockdown.sql` | 套用 migration 後執行：對 `public` 的資料表啟用 RLS（不建 policy）、撤銷 Supabase API 角色的權限 |
| `wrangler.toml` | Worker、Hyperdrive binding、靜態檔（`frontend/dist`）、cron |

Schema 直接用 `backend/migrations`，沒有另外的 migration。

## 和 PostgreSQL 版（`backend/`）的差異

**執行環境**（與 D1 版相同）

- `tokio` 只留 `sync`／`macros`／`rt`。`rt` 模組以 JS 時鐘實作 `Instant`、`sleep`、`timeout`；`std::time::Instant` 在 wasm32 會 panic，不可使用。
- 回應送出後的背景工作（AI 回覆寫入、總結生成）用 `ctx.waitUntil`：瀏覽器斷線後最多再跑 30 秒。
- 卡住的總結由 cron（每 10 分鐘）執行 `recover_orphaned`，取代 PostgreSQL 版啟動時與背景迴圈的回收。
- `reqwest` 換成以 `fetch` 實作的 `http` 模組（同樣的 builder 介面）；AI 串流由 JS 緩衝、每 100 ms 取一次。
- Google 登入自行實作 OIDC（PKCE、state、nonce），state 綁定發起登入的瀏覽器（cookie）。ID token 直接從 token endpoint 經 TLS 取得，依 OIDC Core 3.1.3.7 只驗證 claims，不驗簽章。
- 用戶端 IP 取自 `CF-Connecting-IP`。

**資料庫（Hyperdrive → Supabase）**

- SQL、交易、列鎖（`FOR UPDATE`／`FOR SHARE`）、advisory lock 都與 `backend/` 相同。D1 版為了沒有交易與列鎖而做的條件式寫入與 batch 全部換回後端原本的寫法。
- 每個查詢用 `query_typed`／`execute_typed`：參數帶著型別一起送，一次往返，不需要 prepare（Hyperdrive 的交易模式連線池下 prepared statement 不能跨交易）。型別對應與 SQLx 相同（`i32` → INT4、`Uuid` → UUID、`Vec<String>` → TEXT[]…）。
- Workers 不能跨請求共用 socket，所以連線池只存在一個請求之內（`sqlx::scope`）：查詢或交易借一條閒置連線，沒有才連新的；背景工作用 `sqlx::inherit` 沿用請求的連線池。交易獨佔借來的連線，沒提交就被丟掉時送出 `ROLLBACK` 並關閉該連線。
- **Hyperdrive 的查詢快取必須關閉**（`caching.disabled = true`）。預設會快取 SELECT 最多 60 秒，寫入後馬上讀會讀到舊資料：試行時第一次煙霧測試就因此看不到剛存的 AI 回覆。
- Hyperdrive 連 Supabase 的**直連位址**（`db.<ref>.supabase.co:5432`），不經 Supabase 自己的 pooler（兩層連線池沒有好處）。
- Supabase 會把 `public` schema 開放給它的 Data API（PostgREST）。試行專案對所有資料表啟用 RLS 且不建任何 policy，並撤銷 `anon`／`authenticated` 的權限（含預設權限）；應用程式以資料表擁有者 `postgres` 連線，不受 RLS 影響。

**功能**（與 D1 版相同）

- 移除 cascade 即時語音（WebSocket）；語音的 STT／TTS HTTP 端點保留。
- 密碼雜湊由 Argon2 改成 PBKDF2-SHA256（WebCrypto），格式 `$pbkdf2-sha256$i=N$salt$hash`：使用者密碼 100,000 輪，臨時密碼與示範密碼 1,000 輪。**舊的 Argon2 雜湊無法轉換**，從 PostgreSQL 搬資料時要由管理者重設密碼。
- 下列記憶體狀態是「每個 isolate 各自一份」，多個 isolate 之間不共享：登入限流、AI 用量上限、翻譯頻率限制、API key 冷卻、管理介面的請求日誌、重新接上進行中的回覆。

## 部署

需要 `worker-build`（`cargo install worker-build`）與 Node（`npx wrangler`）。憑證只用環境變數傳入，不寫進檔案：

```bash
export CLOUDFLARE_ACCOUNT_ID=...   CLOUDFLARE_API_TOKEN=...
# 1. 建 Supabase 專案，依序套用 backend/migrations/*.sql，再執行 scripts/supabase_lockdown.sql（RLS＋撤銷 anon／authenticated）
# 2. 建 Hyperdrive（直連位址），並關閉查詢快取；把 id 填入 wrangler.toml
npx wrangler hyperdrive create socrates-chat-trial-pg \
  --connection-string "postgres://postgres:<password>@db.<ref>.supabase.co:5432/postgres" --caching-disabled
cd worker
(cd ../frontend && npm ci && npm run build)          # 產生 ../frontend/dist
npx wrangler secret put OPENAI_API_KEYS              # 另有 GOOGLE_CLIENT_SECRET、ADMIN_EMAILS、ADMIN_INITIAL_PASSWORD
npx wrangler deploy --var APP_BASE_URL:https://<worker>.<subdomain>.workers.dev
```

本機測試：`wrangler.toml` 的 `[[hyperdrive]]` 加 `localConnectionString` 指向 Docker Compose 的 PostgreSQL（連線字串加 `sslmode=disable`），`.dev.vars` 放其餘設定後執行 `npx wrangler dev`。cron 用 `curl "http://localhost:8787/cdn-cgi/handler/scheduled"` 觸發。

## 試行結果（2026-11-11，Supabase 東京 ap-northeast-1）

- wasm gzip 後 1.65 MB（免費方案上限 3 MB；D1 版 1.46 MB）。
- `cargo clippy --target wasm32-unknown-unknown -- -D warnings` 通過；原生單元測試 70 個通過、7 個需要 JS 執行環境（與 D1 版相同），shim 另有 3 個測試。`check_sql.py` 的 167 條靜態 SQL 對試行資料庫 `PREPARE` 全數通過；另有 8 條動態 SQL 未檢查（與後端相同的寫法）。
- 線上煙霧測試 **42/42**（真實 AI，設定同 D1 版）。
- 並行測試 **7/7**：同一學生同時買兩個部件不會透支（每次正好一筆成功、一筆 `insufficient_points`）、最後兩位管理者互相停用只成功一個、兩個教室同時匯入同一新學生只產生一組臨時密碼、30 人整批匯入與重複匯入。這些都靠 PostgreSQL 的列鎖與交易，與 `backend/` 相同。
- 延遲（從開發機量）：與 D1 版相近，一般 API 130–400 ms，AI 回覆約 4–5 秒（大部分是 AI）。
- **CPU（`wrangler tail`）比 D1 版高**：

  | 請求 | D1 版 | Supabase 版 |
  | --- | --- | --- |
  | 不碰資料庫（`/health`） | 1–2 ms | 1–2 ms |
  | 一條連線＋1–2 個查詢（`/ready`、`/api/points`） | 約 3 ms | 6–11 ms |
  | 送出訊息（串流 AI 回覆） | 15–21 ms | 26–40 ms（中位數約 33） |

  - 每條新連線要做一次 PostgreSQL 啟動與 SCRAM-SHA-256 驗證（4096 輪 HMAC 在 wasm 裡算），約 4 ms；之後每個查詢 1 ms 以內。
  - 送出訊息經由請求內的連線池只用 1 條連線，但有 9 個查詢加兩組 `BEGIN`／`COMMIT`，約 13 次往返（D1 版因為用 batch 合併，約 9 次）。
  - 第一版每個交易各開一條連線，每則訊息約 4 條；改成請求內的連線池後只剩 1 條，CPU 只少一點，可見主要成本在往返次數。
  - 要再降，得把 `BEGIN` 與第一個查詢管線化、或把查詢併成更少的語句；這部分還沒做。
- 全部請求的結果都是 `ok`，沒有因 CPU 超時被中止；但送出訊息穩定超過免費方案的 10 ms 約 3 倍，比 D1 版更容易在多人同時使用時遇到 1102 錯誤（還沒驗證）。
- 用量：Hyperdrive 免費方案每天 100,000 次查詢；以每則訊息約 13 次往返估算，一天約 7,000 則訊息。Supabase 免費專案一週沒有活動會暫停。
- 試行站與 NTNU 共用同一把 AI key，試行站的用量會算進同一個額度。
- 未驗證：長回覆的總結能否在 `waitUntil` 30 秒內寫完、Google 登入、語音 STT／TTS、多 isolate 下的限流行為、Supabase 暫停後恢復的行為。
