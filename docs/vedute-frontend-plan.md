# Vedute 前端實作規劃（設計交接 → 各組員 agent 開發）

狀態：草稿，2026-11-23。開 Issue 前由維護者確認分工與組員 GitHub ID。

目標：讓每位組員的 agent（Claude Code、Codex、Cursor）都能照同一套規則，把 Figma 定稿做成一致、高品質、可自動驗證的前端，並走完「實作 → 自動檢查 → 點擊測試 → 自動修正 → 開 PR → review」的流程。

## 0. 現況

| 項目 | 現況 |
| --- | --- |
| 設計 | Figma `zW14xVQcXF8S5rQuD6YzDM` 已定稿大部分頁面（[00] Guideline、[02] 對話、[03] 學生、[04] 成果、[05] 教師與管理者）；規則在 `DESIGN.md`；改動紀錄在 `prototypes/c-interior/ISSUES.md`。 |
| 前端程式 | `frontend/` 已有大部分功能（features/ 下 20 個功能目錄），但仍是舊樣式：Inter／Noto Sans、shadcn 預設色、沒有 Vedute 元件。 |
| CI | Frontend job 跑 oxlint、tsc、vitest、build；沒有 e2e、點擊測試、視覺比對、無障礙檢查。 |
| agent 設定 | `.claude/` 整個在 `.gitignore`，skills（impeccable、gsap-*、hallmark）只在各人電腦上，不共享。 |
| PR #653 | 只差 reviewer 核准（Reviewer approval 失敗是因為還沒有人 approve），其他檢查都通過。 |

## 1. PR #653 合併前要補的

1. **更新 PR 描述**（已完成）：補上 Figma 定稿後新增的規範（DESIGN.md 新增的段落清單：目錄列兩種外框、標題 Title/H3、純圖示按鈕不加框、停用不用透明度、開關五種狀態、各頁定案規格），以及「Figma 為頁面的依據、HTML 原型只代表殼層與動畫」。
2. **列出需要需求方確認的產品面項目**（已寫進 PR 描述）（設計稿先畫了，但屬於產品規格，不能當成已決定）：
   - 隱私設定的「公開開關」：S-02.3／S-07.3 尚未決定，Figma 有現況與「待確認提案」兩組。
   - 管理者總覽的六張區域卡與關鍵數字、學生總結列表的進度卡、題目庫「使用中的活動數」：需要後端提供的資料。
   - 主張與理由頁的「論證標籤」（判準／前提／追問／反例／修正）與 AI 理由群組：對應 S-04.5、F-15。
   - 這些寫進 `docs/specs/open-decisions.md`，或在 PR 描述標成「待確認」。
3. **找一位 reviewer approve**（required check）。

## 2. 設計稿收尾（Figma，可與開發平行）

| 項目 | 說明 |
| --- | --- |
| 尚未重畫的狀態畫面 | R05 統計資料與 R02 活動詳細的空狀態／載入中／讀取失敗；R13／R15／R16 總結產生中、等待、失敗；E51–E55 AI 題目建議；E82–E87 教師管理與系統設定；E06 已封存教室詳細頁；E36 已結束活動參與名單；入口 [01]。 |
| 原型連結 | 新頁面多數已接；「更多」選單、上一題／下一題、桌機已封存頁籤沒有目標畫面。 |
| 清理 | [90] 提案標記「已採用／未採用」；[99] 封存頁保留；刪掉剩下的隱藏舊圖層。 |
| 元件描述 | Figma 元件的 description 補齊用法（Button、Form/Switch、Data/Meta 已有部分）。 |
| Figma 節點對照表 | 產生 `docs/design/figma-map.md`：每個畫面 × 三種尺寸的 node ID 與對應路由，讓 agent 直接用 Figma MCP 讀取。 |

## 3. 前端開發架構

### 3.1 設計 token（先做，所有人依賴）

- `frontend/src/styles/tokens.css`：把 Figma 變數（paper、paper-deep、ink、ink-soft、ink-faint、surface、chalk、chalk-deep、aerugo、ochra、ochra-graphic、caeruleum 與各 wash）寫成 CSS 變數，再用 Tailwind v4 `@theme` 對應成 utility。
- 字型：改用 Noto Serif TC＋IM FELL English SC（@fontsource），移除 Inter 與 Noto Sans。
- 文字樣式：Title/H3、Body、Body Strong、Note 等對應成 Tailwind class 或元件。
- 圓角只有 0／2／圓形；不使用陰影（Toast 用 1.5px 框）。
- 測試：一支 vitest 檢查主要前景／背景 token 組合的對比（文字 ≥ 4.5:1、元件邊界 ≥ 3:1），不需要新套件。

### 3.2 元件層（`src/components/vedute/`）

shadcn/ui 的 `components/ui/` 保留不手改，Vedute 元件包在外面：

| 群組 | 元件 |
| --- | --- |
| 基礎 | Button（刻印 Primary、Quiet、ghost、link、純圖示無框；停用不用透明度）、Switch（on／off／disabled on／disabled off／locked）、Field、Select、Textarea、Tabs（Nav/Tab Item）、StatusChip、Meta（圖示＋文字）、Toast、Alert、Pagination |
| 版畫 | Stamp（TAV.／QVAEST.／ACTVM／CLASSIS，64／48）、DirectoryRow（狀態線型、題目框型）、PlateCard（教室版畫卡）、StageScale（完整與精簡）、BookCover、EditionRow、WorkRow（可展開版本）、StackBar（分段比例長條） |
| 殼層 | Topbar、Sidebar（Nolli 平面圖）、TabBar＋PlanSheet（手機）、PageHeader（版畫切片＋淡彩）、BackHeader（專注情境）、Colophon |

- 開發用的元件總覽頁：`/__design`（只在 dev build 掛載），列出每個元件的所有狀態，取代 Storybook（不新增主要依賴）。
- 動畫照 DESIGN.md（抵達動畫、刻字），用 GSAP；`prefers-reduced-motion` 直接顯示結果。

### 3.3 頁面遷移

依 Figma 節點對照表，一個畫面一張 Issue（三種尺寸一起做），掛在既有的 F-xx Issue 底下，沒有就新開。驗收條件統一：

1. 三種寬度（375、768、1440）與 Figma 一致（版面、文字樣式、token；以視覺比對與人工抽查為準）。
2. 主要操作有點擊測試（Playwright）；既有的 Vitest 行為測試不得退步。
3. axe 無 serious／critical；鍵盤可操作；觸控目標 ≥ 44。
4. 只用 token 與 Vedute 元件，不寫任意色碼、字級。

## 4. 品質與自動化

### 4.1 CI（frontend job 擴充）

| 檢查 | 工具 | 是否需要規格決策 |
| --- | --- | --- |
| lint／typecheck／unit／build | oxlint、tsc、vitest（已有） | 否 |
| 設計規則 lint | 小腳本：禁止 `#hex`、`rgb()`、任意 `text-[..px]`、`opacity-` 用在按鈕停用 | 否 |
| token 對比 | vitest | 否 |
| 點擊測試／e2e | Playwright＋MSW 假後端 | **是**（新增 dev 依賴，S-11 只列了 Vitest＋TL＋MSW） |
| 視覺回歸 | Playwright `toHaveScreenshot`，375／768／1440 | 同上 |
| 無障礙 | @axe-core/playwright | 同上 |

→ 建議先開一張規格 Issue「S-11 補充：前端 e2e、視覺回歸與無障礙檢查採用 Playwright」。

### 4.2 agent 開發流程

1. **共享設定進版控**：`.gitignore` 改成只忽略 `.claude/settings.local.json`、`.claude/worktrees/`，把 `.claude/settings.json`（權限白名單、hooks）、`.claude/skills/`、`.claude/agents/`、`.claude/commands/` 納入版控。
2. **hooks**：編輯 `frontend/` 檔案後自動跑 oxlint＋設計規則 lint；impeccable 的 detector hook 提醒設計問題。
3. **自訂指令**：
   - `/implement-screen <Figma node>`：讀節點、對照 DESIGN.md、先寫測試、實作、三種寬度截圖比對。
   - `/verify-screen <route>`：跑 Playwright 點擊與截圖、axe，回報差異。
4. **AGENTS.md 加「前端與設計」段落**（Codex／Cursor 也讀得到）：設計依據優先序（DESIGN.md → Figma → 原型）、元件與 token 規則、驗收清單、不能做的事（任意色碼、新增主要依賴、改 `components/ui/`）。
5. **PR 範本**加：Figma 節點連結、三種寬度截圖、axe 結果、設計清單勾選。
6. **自動修正與 review**：
   - 個人端：Claude Code 桌面版的 Auto-fix（CI 失敗、review 留言自動修）、`/code-review`。
   - 雲端（需決定）：`anthropics/claude-code-action` 在 PR 自動 review、留言 `@claude` 修正。需要 `ANTHROPIC_API_KEY` secret 與費用決定。

### 4.3 skills 建議

| skill | 建議 | 理由 |
| --- | --- | --- |
| impeccable | **統一安裝** | 設計品質檢查（critique、audit、polish、harden）、DESIGN.md 已用它產生，detector hook 可自動提醒。 |
| gsap-core、gsap-react、gsap-timeline、gsap-performance | **統一安裝** | DESIGN.md 的抵達動畫、刻字、入口轉場都用 GSAP + React。 |
| Figma 官方 plugin（figma-use、figma-design-to-code、figma-implement-motion） | **統一安裝** | 用 Figma MCP 直接讀節點與變數，避免看截圖猜樣式。 |
| run、code-review、simplify（內建） | 使用 | 本機跑起來驗證、PR 前自我 review。 |
| gsap-scrolltrigger、gsap-plugins、gsap-utils、gsap-frameworks | 選用 | 目前沒有捲動驅動或 Vue／Svelte 需求。 |
| hallmark | **不統一** | 用途是「從零設計新網站」，會跟既定的 Vedute 系統衝突。 |
| figma-code-connect | 待定 | Code Connect 需要 Figma 付費方案的 Dev Mode；可先用節點對照表代替。 |

skills 是 Claude 專用；跨 agent 的硬規則一律寫在 AGENTS.md。

## 5. Issue 結構（草案，開之前再確認）

Milestone：**Vedute 前端實作**。

| 編號 | 類型 | 標題 | 相依 |
| --- | --- | --- | --- |
| S-11.x | 規格 | 前端 e2e、視覺回歸與無障礙檢查採用 Playwright | — |
| S-xx | 規格 | 共享 agent 設定、skills 與 AI review 流程 | — |
| F-DS.1 | 功能 | 設計 token、字型與文字樣式 | — |
| F-DS.2 | 功能 | 基礎元件（Button、Switch、表單、Tabs、Chip、Meta、Toast、Alert、Pagination） | F-DS.1 |
| F-DS.3 | 功能 | 版畫元件（Stamp、DirectoryRow、PlateCard、StageScale、書目、StackBar） | F-DS.1 |
| F-DS.4 | 功能 | 殼層（Topbar、Sidebar、TabBar＋PlanSheet、PageHeader、BackHeader、Colophon）＝ ISSUES.md §2 | F-DS.2 |
| F-DS.5 | 功能 | `/__design` 元件總覽頁 | F-DS.2、F-DS.3 |
| T-1 | 工具 | 設計規則 lint 與 token 對比測試 | F-DS.1 |
| T-2 | 工具 | Playwright＋MSW 點擊測試、三寬度截圖、axe，加進 CI | S-11.x |
| T-3 | 工具 | 共享 `.claude/`、hooks、`/implement-screen`、`/verify-screen`、AGENTS.md 前端段落、PR 範本 | S-xx |
| T-4 | 工具 | Figma 節點對照表 `docs/design/figma-map.md` | — |
| P-學生 | 頁面 | 我的教室、教室活動、個人練習、我的對話、點數與任務、我的蘇格拉底、個人檔案三頁籤、活動詳細、對話頁與結束摘要、AI 學習總結 | F-DS.4 |
| P-教師 | 頁面 | 我的教室、教室三頁籤（討論活動、論點分布＋主張與理由、學生名單）、題目庫＋新增題目、共用書庫＋書單、學生帳號、學生總結、對話原文 | F-DS.4 |
| P-管理者 | 頁面 | 總覽、參與概況、教師管理、系統設定 | F-DS.4 |
| P-入口 | 頁面 | 入口 3D 廣場與登入轉場（Three.js、GSAP） | F-DS.1 |

頁面類 Issue 盡量掛在既有的 F-xx 下（例如 F-21 響應式、F-31 列表呈現、F-32 引導問題、F-14.3 書單、F-11.4 公開設定），避免重複。

## 6. 建議順序

1. **第 0 週**：#653 核准合併；開兩張規格 Issue（Playwright、共享 agent 設定）。
2. **第 1 週**：F-DS.1、T-1、T-4 先完成（其他人都依賴）；F-DS.2、F-DS.3 平行；T-2、T-3 同時進行。
3. **第 2–3 週**：F-DS.4 殼層完成後，各組員依分工平行做頁面；設計端同步補完 §2 的狀態畫面。
4. **持續**：每張頁面 PR 附三寬度截圖，由設計負責人 review；視覺回歸基準在合併時更新。

## 7. 開 Issue 前需要決定

1. 組員 GitHub ID 與分工（建議：1 人負責 token＋元件＋工具，其餘依學生／教師／管理者分頁面）。
2. 是否採用 Playwright（規格決策）。
3. 是否把 `.claude/` 共享設定與 skills 納入版控。
4. 是否啟用雲端 AI review（`claude-code-action`）與 API key 的費用。
5. GitHub App（維護者本機的私鑰）的 App ID 與 installation，用來開 Issue 與寫入 org Project。
