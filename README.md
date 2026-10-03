# socrates-chat

蘇格拉底式對話學習系統：學生就哲學題目與 AI 進行多輪討論。系統以追問引導學生釐清立場、檢視理由與假設，而不直接代答；教師可建立討論活動並查看學生的學習成果。系統只服務一門課程，使用資格由修課名單控制。

> **目前狀態：規格階段。** 功能需求已拆成規格（S-xx）與功能（F-xx）Issues，未定案的產品與技術決策都先由規格 Issue 處理。已確認的決策見 [`docs/specs/decisions.md`](docs/specs/decisions.md)。

## 技術棧

| 範圍 | 狀態 | 內容 |
| --- | --- | --- |
| 後端 | 已確定 | Rust、Axum、Tokio、SQLx、Serde、reqwest、tracing |
| 資料庫 | 已確定 | PostgreSQL；開發與 CI 用 Docker Compose，正式環境部署待 S-09 |
| 前端 | **暫定（S-11 尚未正式定案，隨時可改）** | React、TypeScript、Vite、Tailwind CSS、shadcn/ui、assistant-ui |
| AI 服務 | 已確定（S-03.1） | OpenAI API（後端以單一介面包裝，模型待評估） |
| 登入 | 已確定（S-01.1） | 只接受 Google；Rust 自行整合 OAuth，session 存 PostgreSQL |
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
├── frontend/    # 前端（暫定技術選型，見 frontend/README.md）
├── .github/     # CI 與 commitlint workflows
├── AGENTS.md    # 給 AI coding agent 的開發指引
├── CONTRIBUTING.md
└── LICENSE      # MIT
```

## 快速開始

### 用 Docker 一鍵啟動（不需安裝 Rust 或 Node）

需求：Docker。

```bash
cp backend/.env.example backend/.env   # 填入 Google OAuth、OPENAI_API_KEY、OPENAI_MODEL、ADMIN_EMAILS
docker compose --profile app up --build
```

啟動後開啟 http://localhost:3000。這會建置單一映像（後端 API ＋ 前端建置產物，同網域提供）並連同 PostgreSQL 一起啟動，容器內的 `DATABASE_URL`、`FRONTEND_DIR`、`LISTEN_ADDR` 已自動設定，migration 也會自動執行。資料庫存放在 Docker volume（`pgdata`），`docker compose down` 不會遺失資料，加上 `-v` 才會清除。這是本機與試用的便利做法，不代表正式環境的部署方式（S-09 尚未決定）。

### 開發者：分別執行後端與前端

#### 後端

需求：Rust stable（由 `backend/rust-toolchain.toml` 指定，rustup 會自動安裝對應元件）。

```bash
cd backend
cargo run          # 啟動於 http://127.0.0.1:3000
curl http://127.0.0.1:3000/health   # {"status":"ok"}
cargo test         # 執行測試
```

#### 前端

需求：Node.js 與 npm。

```bash
cd frontend
npm ci
npm run dev        # 開發伺服器，/api 轉給本機後端
npm test           # 執行測試
npm run build      # 建置；後端設定 FRONTEND_DIR=../frontend/dist 即可同網域提供
```

細節與約束請見 [`frontend/README.md`](frontend/README.md)。

## 參與開發

請先閱讀 [CONTRIBUTING.md](CONTRIBUTING.md)。重點如下：

- 開發主線為 `main`，**禁止直接 push 到 `main`**，所有改動經由 Pull Request 合併。
- 所有 commit 遵守 [Conventional Commits](https://www.conventionalcommits.org/)，CI 會以 commitlint 檢查。
- 採 SDD + TDD：先確認規格與驗收情境，再寫會失敗的測試，最後做最小實作。

使用 AI coding agent 的開發者，請讓 agent 讀取 [AGENTS.md](AGENTS.md)。

## 授權

本專案以 [MIT License](LICENSE) 授權。所使用的開源套件皆為寬鬆授權；複製進原始碼的 shadcn/ui、assistant-ui 與打包的 Geist 字型所需的聲明見 [docs/THIRD-PARTY-NOTICES.md](docs/THIRD-PARTY-NOTICES.md)。
