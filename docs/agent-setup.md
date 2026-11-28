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

最後一行裝 Playwright 用的瀏覽器。產生或更新基準截圖要用 `npm run test:e2e:linux`，需要 Docker（與 CI 同一個 image，截圖才會一致）。

### 本機跑什麼、交給 CI 跑什麼

不用每次都在本機跑完整 CI。整套 e2e 在 Docker 裡要好幾分鐘，GitHub 的 CI 會在每次 push 後完整跑一次。

| 時機 | 在 `frontend/` 執行 | 為什麼 |
| --- | --- | --- |
| 每次提交前 | `npm run lint && npm run typecheck`，加上改到的測試檔：`npx vitest run <檔案或資料夾>` | 幾秒到一分鐘，能擋掉大部分錯誤 |
| 改到畫面（版面、樣式、元件外觀）時 | `npm run test:e2e:linux -- tests/e2e/<受影響的 spec>.spec.ts --update-snapshots`，看過新截圖再提交 | CI 比對的是 Linux 渲染的基準截圖，macOS 的字型點陣化不同，基準只能在 Docker 裡產生；CI 不會幫你更新基準 |
| 只想快速看自己的畫面 | `npm run test:e2e -- tests/e2e/<spec>.spec.ts` | 在 macOS 直接跑，寫出的 `*-darwin.png` 不進版控 |
| 交給 CI | 完整的 `npm test`、`npm run build`、整套 e2e | push 後自動跑；失敗時看 CI 的 artifact（trace、截圖差異）再修 |

- 只重跑受影響的 spec，不要整套重跑，也不要為了確認穩定而同一支測試反覆跑很多輪；測試不穩定時，修測試本身（例如改成檢查最終結果，不要在動畫進行中取樣），不要靠重跑碰運氣。
- 改了共用元件（`src/components/vedute/`）或 token 時，`/__design` 的截圖（`tests/e2e/vedute-components.spec.ts`）也算受影響，要一起更新。

`npm run lint` 會一起跑設計 lint（`npm run lint:design`），錯誤訊息會直接寫出該用哪個 token 或寫法。

## 2. Claude Code 建議安裝的 skills

依 [`docs/vedute-frontend-plan.md`](vedute-frontend-plan.md) §4.3。skills 是 Claude 專用的輔助，不能取代 AGENTS.md 的規則。

| skill | 建議 | 用途 |
| --- | --- | --- |
| impeccable | **安裝** | 設計品質檢查（critique、audit、polish、harden）；DESIGN.md 就是用它整理的 |
| gsap-core、gsap-react、gsap-timeline、gsap-performance | **安裝** | 抵達動畫、標題刻字、入口轉場都用 GSAP＋React（`useGSAP`） |
| Figma 官方 plugin（含 figma-use、figma-design-to-code、figma-implement-motion） | **安裝** | 透過 Figma MCP 直接讀節點、變數與動態，不看截圖猜樣式 |
| run、code-review、simplify（內建） | 使用 | 本機把 app 跑起來驗證、開 PR 前自我 review |
| gsap-scrolltrigger、gsap-plugins、gsap-utils、gsap-frameworks | 選用 | 目前沒有捲動驅動或 Vue／Svelte 需求 |
| figma-code-connect | **安裝** | 把 Figma 元件對到 `components/vedute`，讀節點時直接拿到該用的元件；全組都是 Figma Education，有 Dev Mode |

## 3. Figma MCP

用 Figma 官方的遠端 MCP server（`https://mcp.figma.com/mcp`），以自己的 Figma 帳號授權。設定方式依各 agent 的官方文件；個人設定不要放進 repo。

常用頁面（檔案 `zW14xVQcXF8S5rQuD6YzDM`）：

| 頁面 | node-id |
| --- | --- |
| [00] Guideline | `0:1` |
| [01] 入口 | `8:2` |
| [02] 對話 | `21:240` |
| [03] 學生 | `21:241` |
| [04] 成果 | `466:702` |
| [05] 教師與管理者 | `21:243` |

每個畫面在三種寬度、各狀態的 frame 節點，以及對應的路由、功能目錄、測試與 Issue，見 [`docs/design/figma-map.md`](design/figma-map.md)。Figma 連結的 `node-id=21-241` 就是 `21:241`。

## 4. Codex 與 Cursor

- 兩者都會讀 `AGENTS.md`；Cursor 若沒有自動讀取，在 Rules 加一條「先讀 AGENTS.md 與 DESIGN.md」。
- 上面的 Claude skills 沒有一對一的對應。支援 SKILL.md 格式 skills 的版本可以安裝同名的 GSAP、Figma skills；沒有的話，動畫數值直接讀 HTML 原型（`prototypes/b-roman-forum/vedute.html`、`prototypes/c-interior/index.html`），設計檢查改由 lint 與截圖驗證負責。
- 不要把 Cursor Rules 或 Codex 設定寫進 repo；有需要所有人遵守的規則，開 PR 改 `AGENTS.md`。

## 5. 驗證畫面

不另外加 `verify:screen` 指令：每個畫面都寫一支 `frontend/tests/e2e/<畫面>.spec.ts`，它就是這個畫面的驗證。寫法見 [`frontend/README.md`](../frontend/README.md)「點擊測試、截圖與無障礙」，重點：

- 從 `./fixtures` 匯入 `test`、`expect`，`loginAs(...)` → `page.goto(...)` → 用角色與文字斷言 → `expectNoA11yViolations(page)` → `expectScreenshot(page, '<畫面>.png')`。
- 畫面要的 API 假資料加在 `src/test/browser.ts` 的 `e2eHandlers`，只用合成資料；不需要後端與資料庫。
- 每支測試自動跑 375、768、1440 三種寬度；axe 的 serious、critical 視為失敗（S-11.6），minor、moderate 會附在報告裡。

```bash
cd frontend && npm run test:e2e:linux -- tests/e2e/<畫面>.spec.ts
```

```bash
cd frontend && npm run test:e2e:linux -- tests/e2e/<畫面>.spec.ts --update-snapshots
```

第一行驗證；第二行在畫面有意改變時更新基準截圖，看過 `tests/e2e/__screenshots__/` 的新圖再提交。只想在 macOS 快速看自己的改動，可用 `npm run test:e2e`（寫出的 `*-darwin.png` 不進版控）。

## 6. 指令範本（一個畫面 Issue）

把下面整段貼給 agent，替換 `<…>`：

```text
實作 GitHub issue #<編號>（用 `gh issue view <編號>` 讀取）。

1. 先讀 AGENTS.md（特別是「前端與設計」）、DESIGN.md 開頭的「設計依據分工」與 Issue 提到的章節、docs/design/figma-map.md 中這個路由的列。
2. 用 Figma MCP 讀 figma-map.md 列出的 frame 節點（1440、768、375 與各狀態都要讀），取得版面、文字樣式與狀態；動畫與材質改看 HTML 原型 <原型路徑與區段>。
3. 先寫會失敗的測試：Vitest＋MSW 的元件測試，以及 frontend/tests/e2e/<畫面>.spec.ts 的 Playwright 點擊測試（含 expectNoA11yViolations、expectScreenshot），涵蓋主要操作、載入（skeleton）、錯誤、空資料與權限情境。只用合成資料。
4. 最小實作：只用 tokens.css 的 token、type-* 文字樣式與 src/components/vedute/ 的元件；不寫原始色碼或任意值，不改 src/components/ui/，不新增主要依賴。文案三種語言都要加。
5. 在 frontend/ 跑 npm run lint、npm run typecheck、改到的 Vitest 測試，以及 npm run test:e2e:linux -- tests/e2e/<畫面>.spec.ts（畫面有意改變時加 --update-snapshots），全部通過（axe 不能有 serious、critical）；完整的 npm test、build 與整套 e2e 交給 CI。把 tests/e2e/__screenshots__/ 的 375／768／1440 截圖與 Figma 對照，列出差異並修正。
6. Figma、DESIGN.md、原型之間不一致或規格沒寫到時，停下來問，不要自行決定。
7. 依 CONTRIBUTING.md 開分支、commit，開 draft PR，填好 PR 範本的「前端與設計」段落（Figma 節點、三寬度截圖、axe 結果、清單勾選），並寫 Closes #<編號>。
```

## 7. PR 與 review 流程

1. 從最新的 `main` 開分支（`feat/f-41.13-student-classrooms` 這類命名，見 [CONTRIBUTING.md](../CONTRIBUTING.md)）。
2. commit 遵守 Conventional Commits；agent 產生的 commit 保留它加的 `Co-Authored-By` 行。
3. 先開 **draft PR**。PR 以 squash 合併，**PR 標題就是 `main` 上的 commit 訊息**，請用英文的 Conventional Commits 標題，例如 `feat(frontend): rebuild student classroom list with Vedute components`。
4. 填 PR 範本：`Closes #<編號>`、前端與設計段落（Figma 節點連結、三寬度截圖、axe 結果、設計清單）。review 重點是這些驗證產出，不是逐行讀 diff。
5. CI 全綠、自己看過截圖後，把 draft 轉成 Ready for review。@whyhugo 負責 review 與合併，不要自己合併，也不要 push 到 `main`。
