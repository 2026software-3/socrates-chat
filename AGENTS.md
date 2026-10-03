# AGENTS.md

給在本 repo 工作的 AI coding agent（Claude Code、Codex、Cursor 等）的指引。人類開發者的完整流程請見 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 專案概述

蘇格拉底式對話學習系統：學生就哲學題目與 AI 多輪討論，AI 以追問引導思考而非代答；教師建立討論活動並查看學生的學習成果。目前處於規格階段，需求以 GitHub Issues 管理：`S-xx` 為規格決策，`F-xx` 為功能縱向切片。兩者都以 sub-issue 往下拆（例如 `S-01.2`、`F-06.1`、`F-06.1-BE`），只實作被指派的葉節點，並遵守它的原生 blocked-by 相依。層級與 label 規則見 [CONTRIBUTING.md](CONTRIBUTING.md#issue-怎麼組織)。

## 目錄結構

| 路徑 | 內容 |
| --- | --- |
| `backend/` | Rust + Axum API 服務（`src/lib.rs` 提供 `app()` router，`src/main.rs` 啟動服務） |
| `backend/tests/` | 後端整合測試 |
| `frontend/` | 佔位；前端技術待 S-11 選型，**不要自行建立前端專案** |
| `.github/workflows/` | CI（後端）與 commitlint |

## 指令

後端（在 `backend/` 執行）：

```bash
cargo fmt --check                  # 格式檢查
cargo clippy --all-targets -- -D warnings   # lint，警告視為錯誤
cargo test                         # 測試
cargo run                          # 啟動於 127.0.0.1:3000
```

提交前，上述 fmt、clippy、test 都必須通過。

## 技術前提（不可超出）

- **已確定**（詳見 [`docs/specs/decisions.md`](docs/specs/decisions.md)）：
  - 後端：Rust、Axum、Tokio、SQLx、Serde、reqwest、tracing；資料庫 PostgreSQL。
  - 登入：只接受 Google，由 Rust 自行整合 OAuth，session 存 PostgreSQL（S-01.1）。
  - AI：OpenAI，後端以單一介面包裝 AI 呼叫，保留更換彈性（S-03.1）。
  - 對話 API：HTTP + SSE 串流（S-08.4）；前端與 API 同網域部署（S-08.1）。
  - 登入驗證與 session 規格見 `docs/specs/S-08.2-auth-verification.md`；維運流程見 `docs/specs/S-09.3-ops-procedures.md`。
  - Migration：`sqlx migrate`（`backend/migrations/`）；設定與秘密用環境變數（S-08.3）。
  - 開發／CI 資料庫：Docker Compose 的 PostgreSQL（S-09.1）。
- **未定案，不得自行決定**：
  - 前端技術（S-11）
  - OpenAI 具體模型（S-03.4 評估後決定）
  - 正式環境的部署方式（S-09）
  - 各套件版本
  - 其他尚未決定的規格（見 `docs/specs/decisions.md` 的「尚未決定」清單）
- 不引入 Supabase、Redis 或其他未列出的基礎設施。
- 只在 Issue 需要時才加入依賴，例如 SQLx 與 reqwest 等到對應功能實作時才加入。

遇到未定案事項時，停下來詢問或在 PR 中標示為待確認，不要把假設寫成規格或驗收條件。

需求方確認了一項規格決策時，必須以 PR 更新 `docs/specs/`（`decisions.md`，必要時另寫詳細規格），並關閉對應的規格 Issue；不要只把決策寫在 Issue 或對話裡。流程見 [CONTRIBUTING.md](CONTRIBUTING.md#規格決策流程)。

## 產品範圍前提

- 系統只服務**一門課程**：**沒有班級**，也不分學期。不要建立班級、學期或多課程相關的資料模型。
- 使用資格由教師或管理者匯入的**修課名單**（學生電子郵件）控制；名單外帳號登入後看到「尚未開通」。
- 角色：學生、教師（可多位，含助教，權限相同）、系統管理者。教師看得到所有學生的對話總結，但**看不到對話原文**。

## 工作方式：SDD + TDD

1. **先讀 Issue 與規格文件。** 確認來源需求、使用者情境、Given / When / Then、權限與失敗情境；Issue 的「規格依據」連到 `docs/specs/`，規則細節以文件為準（Issue 與文件不一致時以文件為準）。規格 Issue（S-xx）尚未關閉時，被它 block 的功能不得開始實作。
2. **先寫會失敗的測試。** 依功能選擇單元、整合、契約或 UI 測試，並確認測試因正確的理由失敗。
3. **最小實作。** 只加入讓測試通過所需的程式，再在本 Issue 範圍內整理。
4. **AI 行為測試。** 使用固定情境與行為評分準則，例如是否追問理由、是否避免代答、是否恰當收斂；不要求輸出逐字相同。需記錄提示或規則版本。
5. **只提交完成該 Issue 所需的改動。** 不順手重構無關程式，也不新增不相關的抽象。

## 架構邊界

- 前端只呼叫 Rust 後端，**不直接存取 PostgreSQL 或 AI 服務**。唯一例外是瀏覽器內建的 Web Speech API（S-05.3 混合模式），辨識出的逐字稿仍要送回後端保存。
- 前端不得持有資料庫憑證或 AI 服務金鑰；金鑰與憑證不得進入版本控制。
- 後端負責驗證登入身分與應用程式權限、呼叫 AI 服務，並以 SQLx 保存對話與結果。
- 後端對資料讀寫與外部 AI 呼叫的錯誤處理都必須可測試。回給前端的錯誤不得包含堆疊、金鑰或內部資訊。

## 資安與個資底線

- 應用程式的管理介面、API、報表、匯出與日誌中，每則訊息內容一律以固定字串 `message` 代替。系統管理者可直接存取資料庫與備份，因此讀得到原文；這是經需求方確認的例外（S-02.1），存取需留紀錄。
- 其他敏感欄位（AI 總結、學生反思、分析結果、語音逐字稿）在管理介面一律遮蔽；教師看不到對話原文（S-02.1、S-02.2）。
- 日誌（tracing）不得記錄密碼、token、金鑰、聊天原文或敏感個資。
- 授權檢查必須有測試證據，至少涵蓋未登入、角色不符、名單外帳號、存取他人資料等情境。
- 測試資料只使用合成資料，不使用真實學生資料。

## Git 規範

- 開發主線為 `main`。**永遠不要 push 到 `main`**，也不要在 `main` 上直接 commit。
- 分支命名為 `<type>/<slug>`；有對應 Issue 時帶上編號，即 `<type>/<issue-id>-<slug>`，例如 `feat/f-06-text-chat`、`docs/s-03-ai-dialogue-spec`；沒有 Issue 的改動可省略編號，例如 `chore/bump-deps`。
- 所有 commit 遵守 [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/)，格式與 scope 見 [CONTRIBUTING.md](CONTRIBUTING.md#commit-規範)。
- PR 描述需以 `Closes #<issue>` 關聯 Issue。
- 不要使用 `--no-verify`、`--force` push 到共用分支，也不要改寫已推送的歷史，除非維護者明確要求。
