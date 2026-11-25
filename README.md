# socrates-chat

蘇格拉底式對話學習系統：學生就哲學題目與 AI 進行多輪討論。系統以追問引導學生釐清立場、檢視理由與假設，而不直接代答；教師可建立討論活動並查看學生的學習成果。系統只服務一門課程，以教室區分不同群的學生，使用資格由各教室的修課名單控制。

> **目前狀態：規格階段。** 功能需求已拆成規格（S-xx）與功能（F-xx）Issues，未定案的產品與技術決策都先由規格 Issue 處理。已確認的決策見 [`docs/specs/decisions.md`](docs/specs/decisions.md)。

## 操作演示

![操作演示：學生與 AI 討論電車難題並產生學習總結，再由教師查看班上論點分布](docs/demo/demo.gif)

學生選題後與 AI 多輪討論，結束時產生 AI 學習總結；教師在「班上論點分布」看到全班的主張分組與論證傾向。完整畫質見 [`docs/demo/demo.mp4`](docs/demo/demo.mp4)，重新錄製方式見 [`scripts/demo/`](scripts/demo/record.mjs)。

## 技術棧

| 範圍 | 狀態 | 內容 |
| --- | --- | --- |
| 後端 | 已確定 | Rust、Axum、Tokio、SQLx、Serde、reqwest、tracing |
| 資料庫 | 已確定 | PostgreSQL；開發與 CI 用 Docker Compose，正式環境部署待 S-09 |
| 前端 | 已確定（S-11） | React、TypeScript、Vite、Tailwind CSS、shadcn/ui、assistant-ui |
| AI 服務 | 已確定（S-03.1） | OpenAI API（後端以單一介面包裝，模型待評估） |
| 登入 | 已確定（S-01.1） | 內建電子郵件＋密碼（不開放註冊）加 Google 登入；Rust 自行實作，session 存 PostgreSQL |
| 對話傳輸 | 已確定（S-08.4） | HTTP + SSE 串流 |

各套件版本與正式環境部署方式尚未指定。Supabase 與 Redis 不在目前架構內。

### 資料流

```mermaid
flowchart LR
  FE[前端] -->|HTTP API| BE[Rust 後端<br/>驗證身分與權限]
  BE -->|SQLx| DB[(PostgreSQL)]
  BE -->|reqwest| AI[AI 服務]
```

前端只呼叫 Rust 後端，不直接存取資料庫或 AI 服務，也不持有資料庫憑證或 AI 服務金鑰。

## 專案結構

```text
.
├── backend/     # Rust + Axum API 服務
├── frontend/    # 前端（見 frontend/README.md）
├── scripts/     # 開發輔助腳本（匯入示範資料、錄製操作演示）
├── .github/     # CI 與 commitlint workflows
├── AGENTS.md    # 給 AI coding agent 的開發指引
├── CONTRIBUTING.md
└── LICENSE      # MIT
```

## 快速開始

```bash
git clone https://github.com/2026software-3/socrates-chat.git
cd socrates-chat
cp .env.example .env    # 填入 GOOGLE_CLIENT_ID、GOOGLE_CLIENT_SECRET、ADMIN_EMAILS、OPENAI_MODEL
# AI API key 不放檔案：啟動後以管理者登入，在「系統 AI」頁設定供應商、模型與 key
```

### 本地執行（需要 Rust、Node.js、Docker）

```bash
# 終端機 1：資料庫＋後端（http://127.0.0.1:3000）
docker compose up -d db
set -a && . ./.env && set +a
cd backend && cargo run
```

```bash
# 終端機 2：前端（http://localhost:5173）
cd frontend
npm ci
npm run dev
```

### Docker（只需要 Docker）

```bash
docker compose --profile app up --build    # http://localhost:3000
```

兩種跑法都使用 3000 埠，同一時間只能跑其中一種。

### 匯入示範資料（選用）

以 `DEMO_MODE=true` 啟動後端一次（會建立示範教師與 10 位學生，帳號與公開的固定密碼見 [`backend/src/demo_accounts.json`](backend/src/demo_accounts.json)，登入頁也會列出來、點一下填入），再執行（全部是合成資料，不需要管理者密碼）：

```bash
./scripts/seed-demo.sh
```

會建立 3 間教室（含 1 間已封存）、5 個題目、9 個活動（草稿、已發布、已停止開放、預定開放，另有 1 個尚無對話、留給現場示範的活動）、約 26 場對話（已完成的含逐則訊息、AI 總結與主張分組；另有進行中、總結產生中與總結失敗的；也有英文與西班牙文的學生對話）。可重複執行，會先清掉上次的示範資料。**示範模式的密碼是公開的，只用在展示環境**，且示範帳號不含管理者。發表流程與講稿見 [`docs/demo-script.md`](docs/demo-script.md)。

### 教師書庫與初始書目匯入

共用書庫以作品整理版本；新增版本只必填書名，先建立獨立作品，再由教師或管理者確認合併、移動版本。所有教師可調整歸組，但只能編輯自己新增的版本內容；作品及版本的停用、重新啟用與永久刪除僅限管理者。題目書單選作品，加入活動時複製為每題各自的書單，後續獨立調整。合併作品會轉移並去重書單關聯；移動版本只改歸屬。刪除最後版本時一併刪作品，刪作品則連同全部版本與書單關聯刪除，操作前均須確認影響。一般外部搜尋建議設定 `GOOGLE_BOOKS_API_KEY`，優先查 Google Books，正常無可用候選才查 Open Library；未設定或留白時直接查 Open Library。有金鑰但 Google 錯誤／逾時仍回報失敗，不切換來源。手動建立不需要金鑰；外部服務失敗不阻擋有效 CSV 書目匯入，該列標示待補資料。同作品其他版本查找直接使用 Open Library，不需此金鑰。

初始書庫由開發團隊以 UTF-8 CSV 匯入，不提供上傳介面。先依既有流程套用 migration；指令不自動載入 `.env` 或執行 migration：

```bash
# 在 repo 根目錄；將 books.csv 換成團隊整理的檔案
set -a
. ./.env
set +a
cd backend
cargo run --locked --bin import-books -- /path/to/books.csv
```

格式見 [books.csv 範例](backend/examples/books.csv)。僅 `title` 必填，多位作者用分號分隔；接受 BOM、引號內逗號與跨行簡介。有效列逐筆儲存；錯誤、重複／疑似重複列不覆寫。只補缺值，多個外部候選或服務失敗仍建立有效書目，於 JSON 報告列行號、書名與待補原因。未設定金鑰時可先匯入，再由管理者補資料。初始匯入每筆先建立獨立作品，不綁題目／活動，也不自動標示教師推薦；重複列不建立額外空作品。

同時只接受一個匯入作業；第二個直接回報忙碌，不排程。匯入中斷時已完成列保留，修正後可重跑並依去重規則跳過已存在書目。使用真實外部服務時會消耗專案查詢額度；相關規則與已知來源限制見 [S-06.3](docs/specs/S-06.3-book-recommendations.md)。

新增的作品 migration 會保留現有書目 ID、版本資料及書單關聯；每筆既有書目先對應獨立作品，不自動判定翻譯關係。

### 錄製操作演示（選用）

匯入示範資料後，用 Playwright 自動操作並錄成 `docs/demo/demo.mp4` 與 `demo.gif`（需要 ffmpeg；學生對話會真的呼叫 AI，產生少量費用）：

```bash
cd scripts/demo
npm install && npx playwright install chromium
npm run record
```

每次錄製都會替學號 `11500001A` 多留一場對話，班上分布的數字會跟著變動。

### 正式環境

正式環境（NTNU，主機 msi_cubi）與 staging 由 GitHub Actions 自動部署，流程與維運見 [`docs/ntnu-ops.md`](docs/ntnu-ops.md)。規格上正式環境的部署方式仍未定案（S-09）；目前 app 啟動時套用 migration（`RUN_MIGRATIONS=true`），與 [S-09.3](docs/specs/S-09.3-ops-procedures.md) 的規格不同，維持現狀直到維運負責人決定。`COOKIE_SECURE=true` 必須保持。

## 參與開發

請先閱讀 [CONTRIBUTING.md](CONTRIBUTING.md)。重點如下：

- 開發主線為 `main`，**禁止直接 push 到 `main`**，所有改動經由 Pull Request 合併。
- 所有 commit 遵守 [Conventional Commits](https://www.conventionalcommits.org/)，CI 會以 commitlint 檢查。
- 採 SDD + TDD：先確認規格與驗收情境，再寫會失敗的測試，最後做最小實作。

使用 AI coding agent 的開發者，請讓 agent 讀取 [AGENTS.md](AGENTS.md)。

## 授權

本專案以 [MIT License](LICENSE) 授權。所使用的開源套件皆為寬鬆授權；複製進原始碼的 shadcn/ui、assistant-ui 與打包的 Geist 字型所需的聲明見 [docs/THIRD-PARTY-NOTICES.md](docs/THIRD-PARTY-NOTICES.md)。
