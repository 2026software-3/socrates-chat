# 貢獻指南

這份文件寫給第一次參與 socrates-chat 的開發者。即使你還不熟悉 GitHub Issues、Project 或本專案的規則，照著讀完也能從「找任務」一路做到「PR 合併」。

建議閱讀順序：

1. [先看這裡](#先看這裡5-分鐘認識專案)：專案是什麼、文件在哪裡
2. [名詞解釋](#名詞解釋)：看不懂的詞先查這裡
3. [第一次設定開發環境](#第一次設定開發環境)
4. [從領任務到合併](#從領任務到合併完整流程)：日常工作流程
5. 其餘各節是細部規則，遇到時再查；最後有[常見問題](#常見問題)

## 先看這裡：5 分鐘認識專案

**這是什麼？** 一個蘇格拉底式對話學習系統：學生就哲學題目與 AI 多輪討論，AI 用追問引導思考而不直接給答案；教師建立討論活動、查看學生的學習成果。系統只服務一門課程，使用資格由修課名單控制。

**目前做到哪裡？**
- 規格階段：大部分規則已決定，少數仍在討論（見 `docs/specs/decisions.md` 的「尚未決定」）。
- 後端：Rust + Axum 的最小骨架，只有 `GET /health`。
- 前端：技術尚未選定（規格 S-11），`frontend/` 目前只是佔位。

**文件地圖**

| 檔案／目錄 | 內容 | 什麼時候看 |
| --- | --- | --- |
| `README.md` | 專案簡介、技術棧、快速開始 | 第一次接觸專案 |
| `CONTRIBUTING.md`（本文件） | 工作流程與規則 | 開始做任何事之前 |
| `docs/specs/decisions.md` | **所有已確認的規則**，以及尚未決定的清單 | 實作前、不確定規則時 |
| `docs/specs/*.md` | 較長的技術設計（架構、登入、維運、技術細節） | 實作相關功能時 |
| `AGENTS.md` | 給 AI coding agent 的指引；人也可以讀，內容是精簡版規則 | 使用 AI 工具協助開發時 |
| `backend/` | Rust 後端 | 後端開發 |
| `frontend/README.md` | 前端目前狀態 | 前端開發（待 S-11 定案） |
| `.github/` | CI、Issue 與 PR 模板 | 開 Issue／PR 時自動套用 |

**三條最重要的規則**

1. **不能直接 push 到 `main`**：一律開分支、開 PR。
2. **規則以 `docs/specs/` 為準**：Issue 只負責討論與追蹤；兩者不一致時，以文件為準。
3. **先寫測試再實作**：每張任務都要先有會失敗的測試。

## 名詞解釋

| 名詞 | 意思 |
| --- | --- |
| Issue | GitHub 上的一張工作單，描述一件要做或要決定的事 |
| 父 Issue／sub-issue | GitHub 原生的階層：一張父 Issue 底下可以掛多張 sub-issue，父 Issue 會顯示子項完成進度 |
| 規格 Issue（`S-xx`） | 需要需求方做決定的事項，例如「前端要用什麼框架」。產出是寫進 `docs/specs/` 的決策，**不寫產品程式碼** |
| 功能 Issue（`F-xx`） | 一個使用者功能，例如「學生進行多輪文字對話」 |
| 功能切片（`F-xx.n`） | 功能裡可以獨立驗收的一小塊，例如「建立對話與送出訊息」 |
| 面向任務（`F-xx.n-BE` 等） | 切片中單一面向的工作：`DB` 資料庫、`AI`、`BE` 後端 API、`SEC` 資安驗證、`FE` 前端 |
| 葉節點 | 沒有 sub-issue 的 Issue，也就是**實際被指派、用 PR 完成**的那張 |
| blocked-by | GitHub 原生的相依關係：「這張要等那張完成才能開始」 |
| Milestone | 交付階段（M1 基礎流程～M7 上線前品質），用來看每個階段的完成進度 |
| Sprint | 2 週一輪的工作週期，在 Project 的 `Sprint` 欄位設定 |
| Status | 任務在 Project 上的狀態：`Backlog`、`Ready`、`In progress`、`In review`、`Done` |
| Issue Type | GitHub 原生的 Issue 類型：`Spec`、`Feature`、`Task`、`Bug` |
| Label | Issue 的標籤，本專案用來標示工作面向（`backend`、`frontend` 等） |
| SDD | Spec-Driven Development：先把規格與驗收條件講清楚，再開始做 |
| TDD | Test-Driven Development：先寫會失敗的測試，再寫剛好讓它通過的程式 |
| Given / When / Then | 驗收情境的寫法：「在什麼前提下（Given），做了什麼（When），應該得到什麼結果（Then）」 |
| Conventional Commits | commit 訊息的格式規範，例如 `feat(backend): add health check` |
| PR | Pull Request：請求把你的分支合併到 `main` |
| CI | 每次 PR 自動執行的檢查（格式、lint、測試、commit 格式） |

## 第一次設定開發環境

1. **取得權限**：請維護者把你加入 GitHub org `2026software-3`。
2. **Clone 專案**

   ```bash
   git clone https://github.com/2026software-3/socrates-chat.git
   cd socrates-chat
   ```

3. **安裝 Rust**：到 [rustup.rs](https://rustup.rs/) 依指示安裝。版本由 `backend/rust-toolchain.toml` 指定，第一次在 `backend/` 執行 `cargo` 時會自動安裝正確版本與 rustfmt、clippy。
4. **確認後端可以跑**

   ```bash
   cd backend
   cargo test                      # 測試全部通過
   cargo run                       # 啟動於 http://127.0.0.1:3000
   curl http://127.0.0.1:3000/health   # 另開終端機，應回傳 {"status":"ok"}
   ```

5. **（建議）安裝 GitHub CLI**：[cli.github.com](https://cli.github.com/)，安裝後執行 `gh auth login`，之後可以用指令開 PR、看 Issue。
6. **（選用）Node.js**：只用來在本機檢查 commit 格式（見 [Commit 規範](#commit-規範)）。

> 前端工具鏈與本機資料庫（Docker Compose 的 PostgreSQL）會在對應的 Issue 完成後補進本節。

## 從領任務到合併：完整流程

以下以一張後端任務 `F-06.1-BE` 為例。

**1. 找一張可以做的任務**

打開 GitHub Project「socrates-chat 開發」：
- 「本次 Sprint」看板：這個 sprint 排定的任務。
- 「待辦總表」：所有未完成的 Issue，依 milestone 分組。

挑一張 **Status 是 `Ready`、Type 是 `Task`、還沒有人被指派** 的 Issue。`Ready` 表示它的 blocked-by 前置都完成了。

**2. 認領**

在 Issue 右側 Assignees 指派自己，並在 Project 上把 Status 改成 `In progress`。

**3. 讀懂要做什麼**

依序讀 Issue 的：
- 「目標」與「Given / When / Then」：要做到什麼；
- 「規格依據」：點進 `docs/specs/` 看規則細節；
- 「明確不包含」：哪些不要做；
- 「相依」：前置是否真的都完成了。

讀完仍不清楚，或發現規格沒定義到的情況，**先在 Issue 留言問**，不要自行假設（見[常見問題](#常見問題)）。

**4. 開分支**

```bash
git switch main && git pull
git switch -c feat/f-06.1-be-send-message
```

分支命名規則見[分支規則](#分支規則)。

**5. 先寫會失敗的測試（TDD）**

依 Issue 的 Given / When / Then 寫測試，執行並確認它**因為正確的原因失敗**（例如回傳 404，而不是編譯錯誤）。可以先 commit 測試：

```bash
git add backend/tests/
git commit -m "test(backend): add failing test for sending a message"
```

**6. 寫剛好讓測試通過的實作**

只做這張 Issue 需要的改動，不要順手重構或加其他功能。

```bash
git commit -am "feat(backend): save student messages before calling AI"
```

**7. 本機檢查**

```bash
cd backend
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

三項都要通過。

**8. 推送並開 PR**

```bash
git push -u origin feat/f-06.1-be-send-message
gh pr create --base main      # 或在 GitHub 網頁上開
```

- PR 描述會自動帶出模板，逐項填寫，第一行寫 `Closes #<Issue 編號>`。
- 把 Project 上的 Status 改成 `In review`。

**9. 通過檢查與 review**

- CI 會檢查格式、lint、測試與 commit 格式，失敗時點進去看原因並修正後再 push。
- 至少一位成員 review；依意見修改後 push，PR 會自動更新。

**10. 合併之後**

- PR 合併時，`Closes #` 會自動關閉 Issue，Project 自動把它移到 `Done`。
- 刪除分支。
- 查看這張 Issue 的「Blocks」清單：如果某張被你擋住的 Issue 現在所有前置都完成了，把它的 Status 從 `Backlog` 改成 `Ready`，讓其他人可以接手。

## 分支規則

- **開發主線是 `main`**，應隨時保持可建置且測試通過。
- **禁止直接 push 到 `main`**，所有改動都從 `main` 開分支、經 PR 合併。
- 分支命名：`<type>/<issue-id>-<slug>`
  - `type` 與 commit type 相同（見下節）；
  - `issue-id` 是 Issue 標題中的編號，改成小寫；
  - `slug` 是幾個英文字的簡短描述，用 `-` 連接。

  ```text
  feat/f-06.1-be-send-message
  fix/f-25.1-roster-import-duplicates
  docs/s-11.1-frontend-framework
  chore/project-bootstrap
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
| `docs` | 只改文件（包括 `docs/specs/` 的規格決策） |
| `style` | 不影響語意的格式調整 |
| `refactor` | 不改變行為的重構 |
| `perf` | 效能改善 |
| `test` | 新增或修正測試 |
| `build` | 建置系統或依賴 |
| `ci` | CI 設定 |
| `chore` | 其他雜項維護 |
| `revert` | 還原先前的 commit |

### Scope（選填，建議使用）

`frontend`、`backend`、`ai`、`db`、`security`、`deps`，也可以使用功能名稱，例如 `auth`、`chat`、`roster`。

### 撰寫原則

- `description` 使用祈使語氣，簡短描述做了什麼，結尾不加句號；中英文皆可。
- 一個 commit 只做一件事。TDD 流程中可以先 commit 失敗的測試（`test: ...`），再 commit 實作（`feat: ...`）。
- 破壞性變更在 type 後加 `!`，或在 footer 寫 `BREAKING CHANGE: <說明>`。
- 用 footer 關聯 Issue，例如 `Refs: #12`。

### 範例

```text
feat(backend): add course roster import endpoint

test(ai): add fixed scenarios for follow-up question behavior

fix(security): mask message content in admin audit export

docs: record decision for S-11.1 frontend framework

feat(backend)!: require session token on chat API

BREAKING CHANGE: /api/chat now returns 401 without a valid session.
Refs: #7
```

### 推送前在本機檢查（選用，需要 Node.js）

```bash
npx --yes -p @commitlint/cli -p @commitlint/config-conventional commitlint --from origin/main
```

沒有輸出錯誤就是通過。最近一個 commit 的訊息寫錯時，可以用 `git commit --amend` 修改（只限還沒 push 的 commit）。

## Pull Request

1. 推送分支並建立指向 `main` 的 PR。
2. 依 PR 模板（`.github/pull_request_template.md`）填寫，至少包含：
   - 對應 Issue：`Closes #<issue>`
   - 改動摘要，以及明確不包含的範圍
   - 測試證據，包括新增的測試與如何執行
   - 規格檢查：改到規則時，已同步更新 `docs/specs/`
3. CI（fmt、clippy、test）與 commitlint 必須通過。
4. 至少一位成員 review 後才能合併。
5. 合併後刪除分支。

原則上**一個 PR 只完成一張葉節點 Issue**，方便 review 與追蹤。

## Issue 怎麼組織

### 階層

需求以 Issue 管理，並使用 GitHub 原生的 sub-issue 分層：

| 層級 | 範例 | 說明 | Issue Type | Labels |
| --- | --- | --- | --- | --- |
| 規格父 Issue | `S-02` | 一組相關的規格決策 | `Spec` | `spec` |
| 規格決策 | `S-02.4` | 單一決策主題 | `Spec` | 面向 + `testing` |
| 功能父 Issue | `F-06` | 使用者情境與整體驗收 | `Feature` | `feature` |
| 功能切片（核心功能） | `F-06.1` | 再往下拆成面向任務 | `Feature` | `feature` |
| 面向任務 | `F-06.1-BE` | 核心功能切片中單一面向的工作 | `Task` | 單一面向 + `testing` |
| 功能切片（其他功能） | `F-11.1` | 本身就是要做的工作 | `Task` | 涉及的面向 + `testing` |

- **實際開發與 PR 都針對葉節點**，PR 以 `Closes #<葉節點>` 關閉；父 Issue 在所有 sub-issue 完成、且整體驗收通過後關閉。
- **面向任務的順序**：`DB`／`AI` → `BE`（先定 API 契約）→ `SEC`、`FE`。前端任務另外要等前端技術選型（S-11）完成。
- 新建 Issue 時請選用模板：「規格決策」、「功能」或「錯誤回報」（`.github/ISSUE_TEMPLATE/`）。

### 相依（blocked-by）

- 以 GitHub 原生的 blocked-by 標示，放在最細的層級。
- **被 block 的 Issue，要等所有前置都關閉後才能開始。** 在 Issue 右側的 Relationships 可以看到它被哪些 Issue 擋住。

### Milestone

- Milestone 代表交付階段：M1 基礎流程、M2 文字對話、M3 教師觀察與資料保護、M4 語音、M5 遊戲化與分享、M6 學習成果與延伸、M7 上線前品質。
- 依相依關係排序，**不代表產品優先順序**。
- 功能父 Issue 與其所有 sub-issue 掛在同一個 milestone；規格 Issue 掛在它最早解除阻擋的階段。一張 Issue 只能屬於一個 milestone。

### Labels

GitHub labels 一律使用英文。類型 label 只用在父 Issue 與中間層；面向 label 採多選，依葉節點實際涉及的工作面向套用，每張葉節點都加上 `testing`：

| Label | 使用範圍 |
| --- | --- |
| `spec` | 規格父 Issue |
| `feature` | 功能父 Issue，以及再往下拆的功能切片 |
| `frontend` | 頁面、互動、狀態呈現、響應式介面 |
| `backend` | API、商業規則、權限判斷及服務流程 |
| `ai` | 模型提示、生成、分類、分析、推薦及評估 |
| `database` | 資料模型、保存、查詢、交易及資料生命週期 |
| `security` | 身分驗證、授權、個資、去識別化、日誌及外部服務資料流 |
| `testing` | 測試設計與自動化；所有 Issue 都包含測試先行工作 |
| `bug` | 錯誤回報 |

`ai`、`database`、`security` 是後端工作的細分面向。在面向任務中各自獨立成一張；在其他功能切片中，依需要與 `backend` 一併套用。

## GitHub Project

所有 Issue 與 PR 都在 org Project「socrates-chat 開發」中追蹤。

### Status 流程

`Backlog` → `Ready` → `In progress` → `In review` → `Done`

| Status | 意思 | 誰來改 |
| --- | --- | --- |
| `Backlog` | 還有未完成的 blocked-by 前置 | 新 Issue 預設 |
| `Ready` | 前置都已完成，可以開始 | 完成前置的人，或維護者 |
| `In progress` | 有人正在做 | 認領的人（同時指派自己） |
| `In review` | PR 已開出，等待 review | PR 作者 |
| `Done` | 完成 | Issue 關閉或 PR 合併時自動移入 |

### Sprint

- 每 2 週一個 sprint（Project 的 `Sprint` 欄位）。
- 只把 `Task` 排進 sprint；`Feature` 與 `Spec` 的進度由其 sub-issue 反映。
- 規格決策（`Spec`）的討論與 sprint 分開進行，在「規格決策」view 追蹤。

### 常用 views

| View | 用途 |
| --- | --- |
| 本次 Sprint | 這個 sprint 的任務看板 |
| 待辦總表 | 所有未完成的 Issue，依 milestone 分組 |
| 我的工作 | 指派給自己、尚未完成的 Issue |
| 規格決策 | 尚未完成的規格 Issue |
| 功能進度 | 各功能父 Issue 與切片的完成進度 |

## 規格決策流程

`docs/specs/` 是專案規則的唯一來源。Issue 負責討論與追蹤，不重複規則細節。

| 位置 | 放什麼 |
| --- | --- |
| `docs/specs/decisions.md` | 所有已確認的決策，以及尚未決定的清單 |
| `docs/specs/<規格>.md` | 較長的技術設計（例如 `S-08.2-auth-verification.md`） |
| 規格 Issue（`S-xx.n`） | 討論尚未決定的事項；決定後內文只留文件連結，並關閉 |
| 功能 Issue（`F-xx`） | 要做的工作與驗收條件；「規格依據」段落連到 `docs/specs/` |

決定一項規格的步驟：

1. 在對應的規格 Issue 討論，並取得需求方確認。
2. 開 PR 更新 `docs/specs/`，commit 類型用 `docs`，並以 `Closes #<規格 Issue>` 關聯。
3. PR 合併後，規格 Issue 自動關閉；被它 block 的功能 Issue 即可移到 `Ready`。

之後要修改已決定的規則，一樣經 PR 修改 `docs/specs/`，並在 PR 說明受影響的功能 Issue。

## SDD + TDD 工作流程

1. **SDD：先固定規格。** Issue 需連結來源需求，描述使用者情境、成功與失敗流程、資料／權限邊界、錯誤或等待狀態，以及可觀察的驗收條件。未定義事項標為待確認，不自行補成產品決策。
2. **TDD：先寫會失敗的測試**，再做最小實作，最後在本 Issue 範圍內整理程式。
3. **AI 功能**：以固定情境與行為評分準則驗收，不要求生成文字逐字相同，並記錄提示／規則版本。
4. **完成條件**：
   - 規格、測試與實作一致，新增測試通過
   - 權限與個資條件有測試證據
   - 必要文件已同步更新

## 資安注意事項

- 不得 commit 金鑰、憑證或 `.env` 檔；本機設定放在 `.env`（已被 `.gitignore` 排除）。
- 測試只使用合成資料，不使用真實學生資料。
- 應用程式的管理介面中，每則訊息以 `message` 代替（系統管理者僅能透過資料庫或備份讀取原文）；日誌不記錄機敏內容。細節見 [AGENTS.md](AGENTS.md#資安與個資底線)。

## 常見問題

**找不到 `Ready` 的任務怎麼辦？**
在「待辦總表」看 `Backlog` 的 Issue 被哪些前置擋住。前置通常是未決定的規格（`Spec`）或尚未完成的任務；可以協助推動該規格的討論，或詢問維護者。

**為什麼前端任務都是 `Backlog`？**
前端技術還沒選定。所有前端任務都 blocked by S-11.5（前端 PoC），S-11 決定並完成 PoC 後才會移到 `Ready`。

**Issue 寫的和 `docs/specs/` 不一樣，聽誰的？**
以 `docs/specs/` 為準，並在 Issue 留言告知，讓維護者修正 Issue。

**做到一半，發現規格沒有定義某種情況？**
不要自行決定。在 Issue 留言描述情況；若需要需求方決定，由維護者開規格 Issue 或更新既有的規格 Issue。其餘部分可以先做，未定義的部分在 PR 中標示為待確認。

**CI 的 commitlint 失敗了？**
代表有 commit 訊息不符合格式。若是最近一個 commit 且還沒被別人拉取，可以 `git commit --amend` 修改訊息後 `git push --force-with-lease`（**只能對自己的分支這樣做**）。不確定怎麼修時，請詢問維護者。

**不小心在 `main` 上 commit 了？**
先不要 push。執行 `git switch -c <新分支名稱>` 把這些 commit 帶到新分支，再依一般流程開 PR；本機的 `main` 之後用 `git switch main && git reset --hard origin/main` 回到遠端狀態（這會丟棄本機 `main` 上未推送的改動，請先確認都已在新分支上）。

**可以一個 PR 完成好幾張 Issue 嗎？**
原則上不行：一個 PR 對應一張葉節點 Issue。若兩張 Issue 真的無法分開，請先在 Issue 留言與維護者確認。

**使用 AI 工具（Claude Code、Codex、Cursor 等）開發可以嗎？**
可以，請讓它讀取 `AGENTS.md`。AI 產生的程式碼一樣要遵守本文件的所有規則，並由你負責確認與測試。
