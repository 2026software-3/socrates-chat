# 貢獻指南

感謝參與 socrates-chat 開發。本文件說明分支、commit、Pull Request 與 Issue 的工作方式。

## 開發環境

| 工具 | 用途 |
| --- | --- |
| Git | 版本控制 |
| [rustup](https://rustup.rs/) | 安裝 Rust；版本由 `backend/rust-toolchain.toml` 指定（stable，含 rustfmt、clippy） |
| GitHub CLI（選用） | 建立 PR、查看 Issue |

前端工具鏈待 S-11 選型後補上。

## 分支流程

- **開發主線是 `main`。** `main` 應隨時保持可建置且測試通過。
- **禁止直接 push 到 `main`。** 所有改動都從 `main` 開分支，經 Pull Request 合併。
- 分支命名為 `<type>/<issue-id>-<slug>`，其中 `type` 與 commit type 相同：

  ```text
  feat/f-06-text-chat
  fix/f-25-expired-invite
  docs/s-01-auth-spec
  chore/project-bootstrap
  ```

- 開始工作前先同步 `main`：

  ```bash
  git switch main && git pull
  git switch -c feat/f-06-text-chat
  ```

## Commit 規範

所有 commit 都必須遵守 [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/)。PR 上的 commitlint 檢查未通過就不能合併。

```text
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

### Type

| type | 用途 |
| --- | --- |
| `feat` | 新功能 |
| `fix` | 修正錯誤 |
| `docs` | 只改文件 |
| `style` | 不影響語意的格式調整 |
| `refactor` | 不改變行為的重構 |
| `perf` | 效能改善 |
| `test` | 新增或修正測試 |
| `build` | 建置系統或依賴 |
| `ci` | CI 設定 |
| `chore` | 其他雜項維護 |
| `revert` | 還原先前的 commit |

### Scope（選填，建議使用）

`frontend`、`backend`、`ai`、`db`、`security`、`deps`，也可以使用功能名稱，例如 `auth`、`chat`、`invite`。

### 撰寫原則

- `description` 使用祈使語氣，簡短描述做了什麼，結尾不加句號；中英文皆可。
- 一個 commit 只做一件事。TDD 流程中可以先 commit 失敗的測試（`test: ...`），再 commit 實作（`feat: ...`）。
- 破壞性變更在 type 後加 `!`，或在 footer 寫 `BREAKING CHANGE: <說明>`。
- 用 footer 關聯 Issue，例如 `Refs: #12`。

### 範例

```text
feat(backend): add class invite code redemption endpoint

test(ai): add fixed scenarios for follow-up question behavior

fix(security): mask message content in admin audit export

docs: clarify data retention decision in S-02

feat(backend)!: require session token on chat API

BREAKING CHANGE: /api/chat now returns 401 without a valid session.
Refs: #7
```

## Pull Request

1. 推送分支並建立指向 `main` 的 PR。
2. PR 描述需包含：
   - 對應 Issue：`Closes #<issue>`
   - 改動摘要，以及明確不包含的範圍
   - 測試證據，包括新增的測試與如何執行
3. CI（fmt、clippy、test）與 commitlint 必須通過。
4. 至少一位成員 review 後才能合併。
5. 合併後刪除分支。

## Issue 與 Labels

需求以 Issue 管理：

- **`S-xx` 規格 Issue**：產出核准的需求／設計決策與 Given / When / Then 情境，不寫產品程式碼。
- **`F-xx` 功能 Issue**：可獨立驗收的縱向切片。被規格 Issue block 時，須等規格核准後才能開始。

GitHub labels 一律使用英文，採多選，依 Issue 實際涉及的工作面向套用。每張 Issue 都加上 `testing`：

| Label | 使用範圍 |
| --- | --- |
| `frontend` | 頁面、互動、狀態呈現、響應式介面 |
| `backend` | API、商業規則、權限判斷及服務流程 |
| `ai` | 模型提示、生成、分類、分析、推薦及評估 |
| `database` | 資料模型、保存、查詢、交易及資料生命週期 |
| `security` | 身分驗證、授權、個資、去識別化、日誌及外部服務資料流 |
| `testing` | 測試設計與自動化；所有 Issue 都包含測試先行工作 |

`ai`、`database`、`security` 是後端工作的細分面向，依需要與 `backend` 一併套用。

## SDD + TDD 工作流程

1. **SDD：先固定規格。** Issue 需連結來源需求，描述使用者情境、成功與失敗流程、資料／權限邊界、錯誤或等待狀態，以及可觀察的驗收條件。未定義事項標為待確認，不自行補成產品決策。
2. **TDD：先寫會失敗的測試**，再做最小實作，最後在本 Issue 範圍內整理程式。
3. **AI 功能**：以固定情境與行為評分準則驗收，不要求生成文字逐字相同，並記錄提示／規則版本。
4. **完成條件**：
   - 規格、測試與實作一致，新增測試通過
   - 權限與個資條件有測試證據
   - 必要文件已同步更新

## 資安注意事項

- 不得 commit 金鑰、憑證或 `.env` 檔。
- 測試只使用合成資料。
- 系統管理者不得看到聊天原文，每則訊息以 `message` 代替；日誌不記錄機敏內容。細節見 [AGENTS.md](AGENTS.md#資安與個資底線)。
