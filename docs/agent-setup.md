# 本機 agent 設定（Vedute 前端）

這份文件寫給用 coding agent（Claude Code、Codex、Cursor）實作 Vedute 前端改版（F-41，#668）的組員。目標是不論用哪個 agent，產出的程式都遵守同一套規則，並能用 lint、測試與截圖自動驗證。

- **硬規則只寫在 [`AGENTS.md`](../AGENTS.md)**（「前端與設計」段落），所有 agent 都會讀；本文件只說明怎麼裝工具與怎麼下指令。
- `.claude/`、`.cursor/`、`.codex/` 等個人 agent 設定**不進版控**，各自在本機設定。
- 設計依據：[`DESIGN.md`](../DESIGN.md)（開頭的「設計依據分工」決定什麼看 Figma、什麼看 HTML 原型）、Figma 檔案 `zW14xVQcXF8S5rQuD6YzDM`、[`docs/vedute-frontend-plan.md`](vedute-frontend-plan.md)。

## 1. 前置條件

```bash
node --version   # 需要 Node 24（與 CI 相同）
```

```bash
cd frontend && npm ci
```

```bash
cd frontend && npx playwright install chromium
```

最後一行在 F-41.7（#675，Playwright＋MSW 點擊測試）合併後才需要；Playwright 的版本以 `frontend/package.json` 為準。

提交前在 `frontend/` 執行：

```bash
npm run lint && npm run typecheck && npm test && npm run build
```

## 2. Claude Code 建議安裝的 skills

依 [`docs/vedute-frontend-plan.md`](vedute-frontend-plan.md) §4.3。skills 是 Claude 專用的輔助，不能取代 AGENTS.md 的規則。

| skill | 建議 | 用途 |
| --- | --- | --- |
| impeccable | **安裝** | 設計品質檢查（critique、audit、polish、harden）；DESIGN.md 就是用它整理的 |
| gsap-core、gsap-react、gsap-timeline、gsap-performance | **安裝** | 抵達動畫、標題刻字、入口轉場都用 GSAP＋React（`useGSAP`） |
| Figma 官方 plugin（含 figma-use、figma-design-to-code、figma-implement-motion） | **安裝** | 透過 Figma MCP 直接讀節點、變數與動態，不看截圖猜樣式 |
| run、code-review、simplify（內建） | 使用 | 本機把 app 跑起來驗證、開 PR 前自我 review |
| gsap-scrolltrigger、gsap-plugins、gsap-utils、gsap-frameworks | 選用 | 目前沒有捲動驅動或 Vue／Svelte 需求 |
| hallmark | **不要用** | 它的用途是從零設計新網站，會和既定的 Vedute 設計系統衝突 |
| figma-code-connect | 先不用 | 需要 Figma 付費方案的 Dev Mode；先用節點對照表 `docs/design/figma-map.md`（F-41.9）代替 |

安裝方式：在 Claude Code 輸入 `/plugin` 搜尋上表的名稱安裝（GSAP 與 Figma 都是官方發布的 plugin）。裝好後輸入 `/skills` 確認列表裡有它們。

## 3. Figma MCP

用 Figma 官方的遠端 MCP server（`https://mcp.figma.com/mcp`），第一次使用時會開瀏覽器以 Figma 帳號授權；帳號需要能檢視設計檔。以下指令以各工具官方文件為準。

- **Claude Code**：安裝 Figma 官方 plugin 即會帶入 MCP server；只想加 MCP 時：

  ```bash
  claude mcp add --transport http figma https://mcp.figma.com/mcp
  ```

  之後在 Claude Code 輸入 `/mcp` 完成授權。
- **Codex**：

  ```bash
  codex mcp add figma --url https://mcp.figma.com/mcp
  ```

- **Cursor**：在使用者層級的 `~/.cursor/mcp.json` 加入下列設定（不要放在 repo 裡的 `.cursor/`）：

  ```json
  { "mcpServers": { "figma": { "url": "https://mcp.figma.com/mcp" } } }
  ```

常用頁面（檔案 `zW14xVQcXF8S5rQuD6YzDM`）：

| 頁面 | node-id |
| --- | --- |
| [00] Guideline | `0:1` |
| [02] 對話 | `21:240` |
| [03] 學生 | `21:241` |
| [04] 成果 | `466:702` |
| [05] 教師與管理者 | `21:243` |

各路由對應的畫面節點見 `docs/design/figma-map.md`（F-41.9）。Figma 連結的 `node-id=21-241` 就是 `21:241`。

## 4. Codex 與 Cursor

- 兩者都會讀 `AGENTS.md`；Cursor 若沒有自動讀取，在 Rules 加一條「先讀 AGENTS.md 與 DESIGN.md」。
- 上面的 Claude skills 沒有一對一的對應。支援 SKILL.md 格式 skills 的版本可以安裝同名的 GSAP、Figma skills；沒有的話，動畫數值直接讀 HTML 原型（`prototypes/b-roman-forum/vedute.html`、`prototypes/c-interior/index.html`），設計檢查改由 lint 與截圖驗證負責。
- 不要把 Cursor Rules 或 Codex 設定寫進 repo；有需要所有人遵守的規則，開 PR 改 `AGENTS.md`。

## 5. 驗證畫面：`verify:screen`

規劃的指令（由 F-41.7 #675 加入 `frontend/package.json`，本 PR 不加，避免與 #675 衝突）：

```bash
cd frontend && npm run verify:screen -- /classrooms
```

預期行為：以 MSW 假資料啟動前端，對指定路由跑 Playwright，產出 375、768、1440 三種寬度的截圖與 axe 報告，有 console 錯誤或 axe 違規時失敗。實際參數與輸出位置以 #675 合併後的 `frontend/README.md` 為準；在那之前，用 `npm run dev` 搭配瀏覽器手動截三種寬度。

## 6. 指令範本（一個畫面 Issue）

把下面整段貼給 agent，替換 `<…>`：

```text
實作 GitHub issue #<編號>（用 `gh issue view <編號>` 讀取）。

1. 先讀 AGENTS.md（特別是「前端與設計」）、DESIGN.md 開頭的「設計依據分工」與 Issue 提到的章節、docs/design/figma-map.md 中這個路由的列。
2. 用 Figma MCP 讀節點 <Figma 連結或 node-id>（三種寬度各自的 frame 都要讀），取得版面、文字樣式與狀態；動畫與材質改看 HTML 原型 <原型路徑與區段>。
3. 先寫會失敗的測試：Vitest＋MSW 的元件測試，以及 frontend/tests/e2e/ 的 Playwright 點擊測試，涵蓋主要操作、載入（skeleton）、錯誤、空資料與權限情境。只用合成資料。
4. 最小實作：只用 tokens.css 的 token、type-* 文字樣式與 src/components/vedute/ 的元件；不寫原始色碼或任意值，不改 src/components/ui/，不新增主要依賴。文案三種語言都要加。
5. 在 frontend/ 跑 npm run lint、npm run typecheck、npm test、npm run build、npm run verify:screen -- <路由>，全部通過；把 375／768／1440 截圖與 Figma 對照，列出差異並修正。
6. Figma、DESIGN.md、原型之間不一致或規格沒寫到時，停下來問，不要自行決定。
7. 依 CONTRIBUTING.md 開分支、commit，開 draft PR，填好 PR 範本的「前端與設計」段落（Figma 節點、三寬度截圖、axe 結果、清單勾選），並寫 Closes #<編號>。
```

## 7. PR 與 review 流程

1. 從最新的 `main` 開分支（`feat/f-41.13-student-classrooms` 這類命名，見 [CONTRIBUTING.md](../CONTRIBUTING.md)）。
2. commit 遵守 Conventional Commits；agent 產生的 commit 保留它加的 `Co-Authored-By` 行。
3. 先開 **draft PR**。PR 以 squash 合併，**PR 標題就是 `main` 上的 commit 訊息**，請用英文的 Conventional Commits 標題，例如 `feat(frontend): rebuild student classroom list with Vedute components`。
4. 填 PR 範本：`Closes #<編號>`、前端與設計段落（Figma 節點連結、三寬度截圖、axe 結果、設計清單）。review 重點是這些驗證產出，不是逐行讀 diff。
5. CI 全綠、自己看過截圖後，把 draft 轉成 Ready for review。@whyhugo 負責 review 與合併，不要自己合併，也不要 push 到 `main`。
