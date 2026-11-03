# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- 學生：修同一門哲學課的學生，在老師指定的哲學題目上與 AI 進行多輪對話（文字或語音），也可以從題目庫做個人練習。
- 教師（含助教，權限相同）：建立教室與討論活動、匯入修課名單，查看學生的學習總結。
- 系統管理者：管理所有教室與帳號、重設密碼。

## Product Purpose

蘇格拉底式對話學習系統：AI 以追問引導學生把理由想清楚，而不是替學生寫答案。成功是學生在對話後能說清楚自己的主張與理由，教師能從總結看見學生的思考。

## Positioning

AI 只追問、不代答。對話以「理由是否站得住」為收斂目標，而不是產出答案。

## Operating Context

- 系統只服務一門課程，不分學期；課程下有多個教室，活動、對話與修課名單屬於教室。
- 入口頁的訪客是名單內的學生、教師與管理者；不在任何教室名單內的帳號登入後看到「尚未開通」。

## Capabilities and Constraints

- 登入：系統內建電子郵件＋密碼（不開放自行註冊，密碼由管理者重設），另支援 Google 登入。
- 教師可查看自己教室學生的 AI 總結，以及已結束活動對話的完整原文（唯讀）；進行中的對話與個人練習看不到（S-02.1，2026-11 修訂）。
- 管理介面、API、報表、匯出與日誌中，每則訊息內容一律以 `message` 顯示。
- 介面三語（S-12.1）。
- 前端已定案：React、TypeScript、Vite、Tailwind CSS、shadcn/ui、assistant-ui；新增主要依賴（例如 GSAP、Three.js）需先經規格討論，入口動畫的依賴尚待確認。
- 規格以 `docs/specs/` 為準。

## Evidence on Hand

- 沒有真實學生資料、使用數據或推薦語；示範內容一律標示為示意。

## Product Principles

- 追問而不代答：任何介面都不暗示 AI 會給答案。
- 登入優先：入口的動畫與探索不能擋住登入。
- 隱私透明：清楚說明誰看得到哪些對話內容。

## Accessibility & Inclusion

- 尊重 prefers-reduced-motion；鍵盤可完成登入；文字對比符合 WCAG AA。
