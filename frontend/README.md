# frontend

> **技術選型已定案（S-11，2026-10-07，見 [`docs/specs/decisions.md`](../docs/specs/decisions.md#s-11-前端技術選型)）：**
> React + TypeScript + Vite + Tailwind CSS + [shadcn/ui](https://ui.shadcn.com)（元件）+ [assistant-ui](https://www.assistant-ui.com)（對話介面）；
> 路由 `react-router`、測試 Vitest + Testing Library + MSW、lint `oxlint`、i18n 自行實作、圖表為純 SVG；入口 3D 與動畫用 Three.js、GSAP（2026-11-20）。
> 不再擴充或更換主要框架；新增主要依賴需先經規格討論。

## 約束（不變）

- 前端只經由同網域的 `/api` 存取 Rust 後端，不直接連線 PostgreSQL 或 AI 服務，也不持有任何金鑰或 token（S-08.1）。
- 認證靠 HttpOnly `sid` cookie；登入是整頁導向 `/api/auth/google/login`，前端不處理 OAuth。
- 後端錯誤只有 `code` 與 `request_id`，沒有已翻譯文字；前端依 `error.<code>` 查翻譯（S-12.1）。
- 對話原文只有擁有者與該教室的教師看得到（教師僅限已結束的活動對話，唯讀）；管理者的畫面不得出現原文（S-02.1）。
- 主要流程需支援桌面、平板與手機尺寸；支援的瀏覽器見 S-10.1。

## 指令

```bash
npm install
npm run dev        # http://localhost:5173，/api 轉給 127.0.0.1:3000 的後端
npm run typecheck  # tsc -b
npm run lint       # oxlint
npm test           # vitest run
npm run build
npm run test:e2e        # Playwright：點擊測試、截圖、axe（本機瀏覽器）
npm run test:e2e:linux  # 同上，但在官方 Playwright Docker image 裡跑（與 CI 相同，比對進版控的基準截圖）
```

後端 API 說明見 [`docs/api/api-reference.md`](../docs/api/api-reference.md)。

## 目錄慣例

```text
src/
  app/          外框、路由彙整、角色守門（Feature / AppRoute / NavItem 型別）
  auth/         目前登入者（AuthProvider、useAuth、useMe）
  components/   共用元件；ui/ 是 shadcn CLI 產生的程式碼，盡量不手改
  features/     每個功能一個目錄：index.tsx（匯出 Feature）、messages.ts（三語翻譯）、頁面與測試
  i18n/         語言、翻譯彙整（zh-TW、en、es；預設 zh-TW）
  lib/          API 客戶端、型別、useFetch
  test/         MSW 假後端（handlers.ts 共用、server.ts 給 Vitest、browser.ts 給 Playwright）與 renderApp 測試輔助
tests/e2e/      Playwright 點擊測試、截圖與 axe（S-11.6）；__screenshots__/ 是基準截圖
```

### 新增一個功能

1. 建立 `src/features/<name>/`，在 `index.tsx` 匯出 `Feature`（路由＋導覽，各自標明 `access`: `member`／`teacher`／`admin`）。
2. 在 `messages.ts` 用 `defineMessages` 寫三種語言的翻譯；鍵名加功能前綴。三種語言的鍵值不一致時 `tsc` 會失敗，`i18n.test.ts` 也會檢查。
3. 把 `Feature` 加進 `src/app/routes.tsx`，把 `messages` 加進 `src/i18n/index.tsx`。
4. 用 `renderApp`＋`mockMe`＋`server.use(http.get(...))` 寫測試；沒被處理的請求會直接讓測試失敗。

### 測試

- 先寫會失敗的測試（AGENTS.md：SDD + TDD）。
- 只用合成資料。
- 授權相關畫面要有測試：未登入、角色不符、名單外帳號（`尚未開通`）。

### 點擊測試、截圖與無障礙（Playwright，S-11.6）

- 測試放在 `tests/e2e/*.spec.ts`。`playwright.config.ts` 會用 `vite --mode e2e`（port 5174）啟動 dev server，
  瀏覽器內的 MSW（`src/test/browser.ts`）代替後端，**不需要後端與資料庫**。`e2e` 以外的模式不會載入 MSW，正式建置也不含它。
- 每個測試跑三種寬度（project `mobile` 375×812、`tablet` 768×1024、`desktop` 1440×900），只用 Chromium。
  只跑一種：`npx playwright test --project=desktop`；除錯：`npx playwright test --ui` 或 `--debug`。
- 第一次在本機跑要先裝瀏覽器：`npx playwright install chromium`。`npm run test:e2e:linux` 需要 Docker。
- 共用 fixture 在 `tests/e2e/fixtures.ts`：
  - `loginAs('student' | 'teacher' | 'admin')`：用合成帳號從登入頁登入（帳號見 `src/test/e2e-accounts.ts`）。
  - `expectNoA11yViolations(page)`：axe 掃整頁，serious／critical 視為失敗。
  - `expectScreenshot(page, '名稱.png')`：等字型載入後整頁截圖比對（動畫關閉、游標隱藏）。
  - 時間固定為 `FIXED_NOW`、時區 Asia/Taipei、語系 zh-TW、減少動態；沒有假資料的 `/api` 請求或未捕捉的例外會讓測試失敗。

#### 新增一個畫面的測試

1. 畫面需要的端點加進 `src/test/browser.ts` 的 `e2eHandlers`（只用合成資料）；Vitest 也用得到的預設值放 `src/test/handlers.ts`。
2. 在 `tests/e2e/` 新增 `<畫面>.spec.ts`，從 `./fixtures` 匯入 `test`、`expect`：`loginAs` → `page.goto(...)` → 用角色與文字斷言 →
   `expectNoA11yViolations(page)` → `expectScreenshot(page, '<畫面>.png')`。
3. `npm run test:e2e:linux` 產生 Linux 基準截圖，看過 `tests/e2e/__screenshots__/` 裡的新圖再一起提交。

#### 基準截圖

- 只有 `*-linux.png` 進版控；CI（`.github/workflows/ci.yml` 的 frontend job）在同一個 Docker image（`linux/amd64`）裡比對，
  不會因為作業系統或字型不同而誤判。CI 不會寫入新的基準：缺基準或有差異都會失敗，報告、trace 與差異圖會上傳成 `playwright-report` artifact。
- 畫面**有意**改變時更新基準，並在 PR 裡說明：

  ```bash
  npm run test:e2e:linux -- --update-snapshots
  ```

- 在 macOS 直接跑 `npm run test:e2e` 時，第一次會寫出 `*-darwin.png`（已被 `.gitignore` 排除），之後拿來比對自己的改動；
  要重來就刪掉這些檔案。
