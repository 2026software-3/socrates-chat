# e2e：黑箱端對端測試

獨立於 `backend/`、`frontend/` 的測試套件，依 `docs/specs/` 的規格驗證**真實執行的系統**：

```text
測試（Node 內建 test runner、fetch）──HTTP──▶ 真正的後端執行檔 ──▶ 全新的 PostgreSQL 資料庫
                                                  └──▶ 假的 OpenAI 伺服器（e2e/lib/fake-openai.mjs）
```

- **零依賴**：只用 Node（>= 22）內建的 `node:test`、`fetch`，不新增任何套件。
- **黑箱**：只透過 HTTP API 操作，資料庫只用 `psql` 做斷言與「調整時間」之類的測試手段（例如把 session 的 `last_seen_at` 改成 8 天前）。
- **隔離**：每個測試檔各自建立一個新資料庫與後端程序，可平行執行，結束後自動清除。
- **決定性**：AI 一律由假伺服器回應，可以精準模擬失敗、逾時、串流中斷；不會呼叫真實 OpenAI。

## 執行

需要：Node、`psql`、一個能建立資料庫的 PostgreSQL（`docker compose up -d --wait` 即可），以及建置好的後端。

```bash
(cd backend && cargo build)            # 產出 backend/target/debug/socrates-chat-backend
cd e2e && npm test                     # 全部（約 1 分鐘）
node --test tests/dialogue.e2e.mjs     # 只跑單一檔案
```

| 環境變數 | 預設 | 說明 |
| --- | --- | --- |
| `E2E_ADMIN_DATABASE_URL` | `postgres://socrates:socrates@127.0.0.1:5432/postgres` | 用來建立／刪除測試資料庫的連線 |
| `E2E_BACKEND_BIN` | `backend/target/debug/socrates-chat-backend` | 後端執行檔 |
| `PSQL` | `psql` | psql 路徑 |

## 規格對照

| 檔案 | 規格 |
| --- | --- |
| `auth.e2e.mjs` | S-01.1、S-01.2、S-08.2：內建登入、密碼規則、限流、session、CSRF、Google 登入入口 |
| `roles-roster.e2e.mjs` | S-01.3、S-01.4、S-02.5、S-08.2 §3 §5：角色、修課名單、停用／復原、刪除學生帳號、稽核日誌 |
| `dialogue.e2e.mjs` | S-03.2、S-03.3、S-03.5、S-08.4：三階段追問、回合上限、AI 失敗／逾時／重試、SSE |
| `dialogue-defaults.e2e.mjs` | S-03.3 預設值（15／20 回合）、cookie `Secure` 預設、語音備援未設定 |
| `catalog-privacy.e2e.mjs` | 題目與活動、S-02.1／S-02.2（教師看不到原文、管理者遮蔽）、對話存取控制、日誌不含敏感資料 |
| `summary-dashboard.e2e.mjs` | AI 總結（失敗重試）、S-04.2 六維分數、個人與班級儀表板、主張分組 |
| `platform-voice.e2e.mjs` | S-08.1 同網域與統一錯誤格式、S-05.3／S-05.4 語音備援 |
| `pending-specs.e2e.mjs` | 規格已確定但尚未實作的項目（S-07、S-12.1、S-02.3、S-06、`/ready`…），以 `todo` 列出 |

功能實作完成後，把 `pending-specs.e2e.mjs` 對應的 `todo` 改寫成真正的測試，放進該主題的檔案。

## 已知限制

- **不涵蓋 Google 登入成功路徑。** 後端啟動時會向 `accounts.google.com` 探索 OIDC 設定（`main.rs` 的 `GoogleIdentity::discover`），而且 issuer 寫死在 `identity.rs`，e2e 無法換成假的 Google 伺服器。因此：
  - 執行 e2e 的環境**必須能連上網際網路**（GitHub Actions 可以；離線時後端無法啟動）。
  - 只涵蓋 Google 登入的入口（導向參數、state／PKCE 暫存、callback 錯誤處理）。`sub` 綁定、`email_verified`、ID token 驗證等情境由 `backend/tests/` 的 Rust 測試負責。
  - 若日後要完整涵蓋，需讓 issuer URL 可由環境變數設定（小幅產品改動），屆時再另開 issue。
- 不含瀏覽器 UI 測試（需要新增瀏覽器自動化依賴，依 AGENTS.md 須先經規格討論）與效能／負載測試（S-10）。
- 逾時類測試使用縮短的設定（首 token 1 秒、完整回覆 3 秒），預設值（15／60 秒）不實測。

## CI

`.github/workflows/e2e.yml` 是**資訊性檢查，不阻擋合併**：job 設 `continue-on-error: true` 與 3 分鐘逾時，失敗或逾時都只顯示警告。部分規格尚未實作，失敗是預期中的事；不要把它設為必要檢查（required check）。
