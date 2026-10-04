# frontend

> **技術選型為暫定（S-11 尚未正式定案）：** React + TypeScript + Vite + Tailwind CSS + [shadcn/ui](https://ui.shadcn.com)（元件）+ [assistant-ui](https://www.assistant-ui.com)（對話介面）。
> 其餘工具（路由 `react-router`、測試 Vitest + Testing Library + MSW、lint `oxlint`）是搭建骨架時的暫定選擇，
> S-11.2～S-11.5 正式討論時可調整。

## 約束（不變）

- 前端只經由同網域的 `/api` 存取 Rust 後端，不直接連線 PostgreSQL 或 AI 服務，也不持有任何金鑰或 token（S-08.1）。
- 認證靠 HttpOnly `sid` cookie；登入是整頁導向 `/api/auth/google/login`，前端不處理 OAuth。
- 後端錯誤只有 `code` 與 `request_id`，沒有已翻譯文字；前端依 `error.<code>` 查翻譯（S-12.1）。
- 對話原文只有擁有者看得到；教師與管理者的畫面不得出現原文（S-02.1）。
- 主要流程需支援桌面、平板與手機尺寸；支援的瀏覽器見 S-10.1。

## 指令

```bash
npm install
npm run dev        # http://localhost:5173，/api 轉給 127.0.0.1:3000 的後端
npm run typecheck  # tsc -b
npm run lint       # oxlint
npm test           # vitest run
npm run build
```

後端 API 說明見 [`docs/api/mvp-backend-api.md`](../docs/api/mvp-backend-api.md)。

## 目錄慣例

```text
src/
  app/          外框、路由彙整、角色守門（Feature / AppRoute / NavItem 型別）
  auth/         目前登入者（AuthProvider、useAuth、useMe）
  components/   共用元件；ui/ 與 assistant-ui/ 是 shadcn CLI 產生的程式碼，盡量不手改
  features/     每個功能一個目錄：index.tsx（匯出 Feature）、messages.ts（三語翻譯）、頁面與測試
  i18n/         語言、翻譯彙整（zh-TW、en、es；預設 zh-TW）
  lib/          API 客戶端、型別、useFetch
  test/         MSW 假後端與 renderApp 測試輔助
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
