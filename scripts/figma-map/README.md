# figma-map

產生 [`docs/design/figma-map.md`](../../docs/design/figma-map.md)：Figma [01]–[05] 每個畫面 ↔ 路由 ↔ 功能目錄 ↔ 既有測試 ↔ F-41 Issue。只用 Node 內建模組，不需安裝套件。

| 檔案 | 內容 |
| --- | --- |
| `frames/*.txt` | 從 Figma 唯讀匯出的區塊與 frame 清單（每頁一份） |
| `dump-frames.figma.js` | 匯出用的 Figma Plugin API 片段（只讀，不修改檔案） |
| `screens.mjs` | 人工維護的對照：畫面怎麼比對 frame、路由、功能目錄、測試、負責 Issue、哪些狀態尚未重畫或只是提案 |
| `build.mjs` | 讀上面兩者，產生文件 |

## 更新

1. Figma 有新畫面或改名：把 `dump-frames.figma.js` 的 `PAGE_ID` 換成該頁（[01] `8:2`、[02] `21:240`、[03] `21:241`、[04] `466:702`、[05] `21:243`），整段交給 Figma MCP 的 `use_figma` 執行，回傳的文字原樣覆蓋 `frames/<頁>.txt`。
2. 路由、測試或負責 Issue 有變：改 `screens.mjs`。
3. 重新產生並檢查：

```bash
node scripts/figma-map/build.mjs
```

```bash
node scripts/figma-map/build.mjs --check
```

`--check` 在以下情況失敗（exit 1）：

- 有 frame 沒對到任何畫面，或同一狀態同一寬度有兩個 frame。
- `screens.mjs` 列出的檔案不存在。
- 路由對不上前端：對照表寫的路由不在前端路由定義裡，或前端有路由沒出現在對照表。路由從 `app/routes.tsx`（含其 `features` 陣列註冊的功能模組）與 `classroomRoutes()` 讀出；導向舊網址的 `Navigate` 不算。
- 有管理者版本的畫面，路由沒列 `/admin`。
- 文件不是最新。

CI（`.github/workflows/figma-map.yml`）在 `frontend/src/**`、`scripts/figma-map/**` 或文件有改動時跑 `--check`。前端 PR 搬檔或改名而失敗時，照錯誤訊息改 `screens.mjs`，再重新產生文件。

## frame 名稱怎麼對到同一列

`build.mjs` 的 `normalize()` 拿掉寬度標記（`-t`、`-m`、`桌機｜`、`[02] RWD｜Tablet｜`、`（平板）` 等），寬度改由 frame 實際寬度判斷（≥1024 桌機、500–1023 平板、<500 手機）。桌機與平板／手機命名不同的狀態寫在 `screens.mjs` 的 `aliases`。
