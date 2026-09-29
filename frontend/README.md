# frontend

> **佔位目錄：前端技術尚未選型，請見 S-11「確認前端技術選型」。**

選型定案前，請不要在此建立前端專案或安裝套件。

## 原草案候選組合（僅供參考）

React、TypeScript、Vite、Tailwind CSS、assistant-ui（對話介面）、Recharts（雷達圖等圖表）。

## 已確定的約束

- 前端只經由 Rust 後端的 HTTP API 存取資料與 AI 功能，不直接連線 PostgreSQL 或 AI 服務。
- 前端不得持有資料庫憑證或 AI 服務金鑰。
- 主要流程需支援桌面、平板與手機尺寸，瀏覽器支援範圍依 S-10。

## S-11 定案後

1. 由對應 Issue 建立前端專案骨架與測試工具。
2. 在 `.github/workflows/ci.yml` 加入前端 job（lint、typecheck、test、build）。
3. 更新根目錄 README.md 與 AGENTS.md 的技術棧與指令。
