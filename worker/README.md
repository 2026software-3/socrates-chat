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
- **沒有互動式交易**：`begin()`／`commit()` 是空操作，每個語句各自提交。必須一起成立的寫入改用 `Pool::batch`（D1 batch 是原子的）；可能失敗的檢查一律移到寫入之前。
- **沒有列鎖**（`FOR UPDATE`／`FOR SHARE`／advisory lock 已移除）。「先讀、檢查、再寫」之間可能插進別的請求，必要的條件改放進寫入語句本身（`UPDATE … WHERE 條件`、`INSERT … SELECT … WHERE 條件`）。逐一檢查結果見下方「交易逐一檢查」。
- SQLite 對欄位上的唯一索引只回報欄位清單，啟動時以 `sqlx::register_constraint_aliases` 對應回 PostgreSQL 的約束名稱。
- 台北時間的每日／每週任務區間改在 Rust 計算（UTC+8，無夏令時間）。

**功能**

- 移除 cascade 即時語音（WebSocket）；語音的 STT／TTS HTTP 端點保留。
- 密碼雜湊由 Argon2 改成 PBKDF2-SHA256（WebCrypto），格式 `$pbkdf2-sha256$i=N$salt$hash`：使用者密碼 100,000 輪，臨時密碼與示範密碼 1,000 輪。**舊的 Argon2 雜湊無法轉換**，從 PostgreSQL 搬資料時要由管理者重設密碼。
- 下列記憶體狀態是「每個 isolate 各自一份」，多個 isolate 之間不共享：登入限流、AI 用量上限、翻譯頻率限制、API key 冷卻、管理介面的請求日誌、重新接上進行中的回覆。

## 交易逐一檢查（2026-11-10，32 處 `begin()`）

D1 的保證只有兩個：**單一語句是原子的**，**一個 batch 是原子的**（全部成功或全部不寫入），兩者都依序執行、不會與其他寫入交錯。語句之間沒有隔離，寫入後別的請求立刻看得到。所以每一處都看兩件事：

- **並行**：讀到的值在寫入前被別的請求改掉，PostgreSQL 版靠鎖避免。修法是把條件放進寫入語句。
- **寫一半**：前面的寫入成功、後面的失敗，PostgreSQL 版靠回滾避免。修法是放進同一個 batch。這種情況只在資料庫本身出錯時發生，使用者操作觸發不了。

結果欄的意思：「修正」是這次已改；「無風險」是不需要鎖或回滾；「接受」是保留的風險，需要真的採用時再處理。

| 位置 | 流程 | PostgreSQL 版靠什麼 | D1 版 | 結果 |
| --- | --- | --- | --- | --- |
| `purchase.rs` `buy` | 扣點購買 | 鎖學生與商品後查餘額 | 餘額、部件啟用、價格三個條件與扣點寫在同一個 `INSERT … SELECT … WHERE`（`points::record_purchase_if_affordable`） | **修正**：原本同時買兩個不同部件會透支。已以並行請求驗證，3 次都只成功 1 筆 |
| `accounts.rs` `set_status` | 停用／啟用帳號 | 鎖住所有管理者 | 「仍有其他啟用中的管理者」放進 UPDATE 的條件 | **修正**：原本兩位管理者同時互相停用，可能一位都不剩。已驗證只會成功 1 位。停用後刪除 session、寫稽核是後續語句，只有資料庫錯誤時會漏 |
| `accounts.rs` `delete_student` | 刪除學生帳號 | 鎖住帳號 | 「不是管理者」「完全屬於自己的教室」放進 DELETE 的條件 | **修正**：原本檢查後學生剛被加進別位教師的教室，仍會被刪 |
| `accounts.rs` `delete_with_audit` | 刪除教室或活動 | 鎖列後計數再刪除 | 稽核（含刪除前的對話數）與刪除在同一個 batch | **修正**：計數準確，不會漏記稽核 |
| `auth_store.rs` `link_google` | 學號帳號連結 Google | 鎖住帳號 | 「信箱不在教師名單、沒被別的帳號使用」放進 UPDATE 的條件 | **修正**：原本檢查後信箱剛被加進教師名單，學生帳號會帶上教師信箱，之後用 Google 登入就有教師權限 |
| `roster.rs` `import_roster` | 匯入修課名單 | 一個交易加 advisory lock | 帳號、稽核、名單整批放在一個 batch；建立帳號用 `ON CONFLICT (student_id) DO NOTHING`，臨時密碼只回傳真的新建立的帳號 | **修正**：原本中途失敗時，已建立帳號的一次性臨時密碼會遺失；兩個教室同時匯入同一學號會整批失敗。已驗證並行匯入兩邊都成功、只回 1 組密碼，30 人匯入後重匯冪等 |
| `catalog.rs` `update_activity` | 修改活動題目 | 鎖活動列，讓開始對話等待 | 記下檢查時的對話數；更換題目的 batch 每句都要求對話數沒變，否則回 `activity_topic_locked` | **修正**：原本檢查「沒人開始」之後剛好有學生開始對話，題目仍會被換掉（違反 S-04.7）。題目以外的欄位是後續語句，只有資料庫錯誤時會漏 |
| `chat.rs` `create` | 開始對話 | 對活動與教室取共享鎖 | 寫入當下再確認題目仍屬於活動、活動已發布、活動與教室都未封存（`INSERT … SELECT … WHERE`） | **修正**：與上一列是同一個競態的另一側。**接受**：開放時段與修課名單沒有在寫入時重查，剛好在關閉或移出名單的瞬間仍可能開始一場對話 |
| `reply.rs` `save_reply` | 保存 AI 回覆 | 鎖住對話列 | 回覆與階段更新在同一個 batch，兩句都要求對話仍是 active | **修正**：原本 AI 回覆佔用逾時後被結束的對話，仍會被寫入回覆 |
| `summary.rs` `end` / `end_conversation` | 結束對話 | 交易 | 結束與建立總結列在同一個 batch | **修正**：原本第二句失敗時，對話已結束卻沒有總結列，學生無法重試。**接受**：封存檢查與結束之間教室被封存 |
| `summary.rs` `generate` | 總結完成、發點、記完成 | 交易 | 總結 ready、對話點數、完成紀錄在同一個 batch，後兩句以「這一代總結已 ready」為條件 | **修正**：原本總結 ready 後點數寫入失敗，就永遠補不回來。成就判定是後續步驟，失敗時下一次完成對話會補上 |
| `claims.rs` `regroup` | 重新分組主張 | 交易 | 刪除舊群組與建立新群組在同一個 batch | **修正**：原本 AI 給出同名群組撞到唯一約束時，會留下舊分組已刪、新分組只建一半 |
| `catalog.rs` `set_archived` | 封存／復原活動 | 鎖住活動列 | `UPDATE … WHERE is_archived <> 新值`，以影響列數決定要不要寫稽核 | **修正**：原本同時封存兩次會記兩筆稽核 |
| `achievements.rs` `unlock_matching`（`record_*`、`backfill` 共 5 處） | 解鎖成就與發點 | 鎖學生，避免兩個並行完成都讀到舊計數而跳過門檻 | 解鎖與點數在同一個 batch | **修正**：原本解鎖成功、點數失敗後，這一級的點數永遠補不回來。並行計數**無風險**：D1 沒有隔離，最後計數的請求一定看到所有完成紀錄 |
| `achievement_admin.rs` `create` | 建立成就 | 交易 | 寫入前先查識別碼；各級在同一個 batch | **修正**：不會只建立部分等級。**接受**：建立後的追溯解鎖與稽核是後續步驟，資料庫錯誤時追溯可能漏掉，管理者調低門檻或重新啟用時會再跑 |
| `achievement_admin.rs` `update` | 修改成就 | 鎖住該成就各級 | 寫入前先驗證合併後的門檻嚴格遞增；各級與共用欄位在同一個 batch | **修正**：不會只改一半。**接受**：兩位管理者同時修改同一成就時，各自以舊值驗證，合起來的門檻可能不遞增 |
| `tasks.rs` `claim` | 領取任務獎勵 | 鎖學生與任務 | 不變 | **無風險**：帳本唯一約束保證只入帳一次；完成進度只增不減。「首次領獎」成就是後續步驟，漏掉時下一次領獎會補上 |
| `summary.rs` `retry` | 重試總結 | 交易 | 不變（單一條件式 UPDATE） | **無風險** |
| `chat.rs` `submit_message` | 送出訊息 | 鎖住對話列 | 回合 claim 是單一條件式 UPDATE；「上一則未回覆」的檢查移到 claim 之前 | **無風險**（並行）。**接受**：claim 之後學生訊息寫入失敗（資料庫錯誤）時，對話會顯示「AI 回覆中」直到 claim 逾時 |
| `auth_store.rs` `upsert_user` | Google 登入建立／綁定帳號 | 交易（沒有鎖） | 不變 | **接受**：同一個 Google 帳號同時第一次登入時，後到的撞唯一約束、這次登入失敗，重試即可（PostgreSQL 版相同） |
| `catalog.rs` `create_activity` | 建立活動 | 交易 | 活動與題目在同一個 batch | **接受**：封存檢查與建立之間教室被封存，會多一個活動 |
| `classrooms.rs` `update_classroom` | 修改／封存教室 | 鎖住教室列 | 不變 | **接受**：同時封存兩次可能記兩筆稽核；封存的瞬間改名可能成功 |
| `roster.rs` `remove_from_roster` | 移出名單 | 共享鎖 | 不變 | **接受**：封存的瞬間仍可能移出一人；稽核是後續語句 |
| `purchase.rs` `update`、`tasks.rs` `update` | 管理者改價格、任務設定 | 鎖住該列 | 不變 | **接受**：兩位管理者同時修改時，稽核的舊值可能是過時的。改價與購買之間的競態已由購買的條件式寫入擋住 |
| `auth.rs` `reset_password`、`roster.rs` `add_teacher`、`delete_email` | 重設密碼、新增／移除教師 | 交易 | 不變 | **接受**：單一寫入加稽核，只有資料庫錯誤時會漏記稽核 |

驗證：`scripts/check_sql.py` 檢查 175 條靜態 SQL 全數通過；本機以 `wrangler dev` 跑並行測試（同時購買、最後兩位管理者互相停用、兩個教室同時匯入同一學號、30 人匯入後重匯）7/7 通過，煙霧測試 42/42 通過。

部署到 workers.dev 後（2026-11-11，commit `c52103d`）：並行測試 7/7 通過；煙霧測試 39/42，失敗的 3 項都需要真的 AI key。`wrangler tail` 只收到 12 筆事件，全部 `ok`、沒有超時中止，CPU 中位數 8 ms；30 人名單匯入 28 ms；更新 secret 後新 isolate 的第一次登入（冷啟動加建立管理者）110 ms。

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
