# 入口視覺提案 prototypes

蘇格拉底・教室入口（登入前的公開頁與登入流程）的視覺提案。全部是單一 HTML 檔，直接用瀏覽器打開即可（需連網載入 CDN 上的函式庫與字型）。帳號與資料皆為示意，與正式前端程式無關。

**決定（2026-11-03）：採用 Vedute（廣場版畫）作為整體主視覺**，規範見根目錄 [DESIGN.md](../DESIGN.md)。其他提案保留作紀錄，不再維護。

| # | 提案 | 檔案 | 繪製方式 | 狀態 | 線上版 |
| --- | --- | --- | --- | --- | --- |
| 1 | A 追問廣場 | [a-plaza/index.html](a-plaza/index.html) | DOM＋SVG＋GSAP；拱門登入卡、對話舞台（多邊形磨成圓）、穿門轉場 | 未採用 | [artifact](https://claude.ai/artifact/FYKP7Gjxj7X2U9P1rGNnNe) |
| 2 | B v1 貼紙風 | [b-roman-forum/sticker-v1.html](b-roman-forum/sticker-v1.html) | Three.js toon＋白邊外殼 | 未採用 | [artifact](https://claude.ai/artifact/Njp2pXif51hPouVrPbP4NP) |
| 3 | **B Vedute 廣場版畫** | [b-roman-forum/vedute.html](b-roman-forum/vedute.html) | Three.js 世界座標排線著色＋墨線外殼；版痕框、編號圖例即導覽 | **採用** | [artifact](https://claude.ai/artifact/6S2FYqPsM8gEf9qQVZQKjt) |
| 4 | B Moneta 鑄幣廠 | [b-roman-forum/moneta.html](b-roman-forum/moneta.html) | Three.js 金屬 PBR；廣場是錢幣坯上的浮雕，每輪追問一記錘擊 | 未採用 | [artifact](https://claude.ai/artifact/NaYS3XtAmhAqhZuhbhBT4X) |
| 5 | B Salve 門檻馬賽克 | [b-roman-forum/salve.html](b-roman-forum/salve.html) | Three.js＋全畫面鑲嵌磚後製 | 未採用 | [artifact](https://claude.ai/artifact/Nt46yvQSqZoHcVvWqtgUwN) |

線上版是 claude.ai 上的私人 artifact，分享前要由擁有者在頁面的 Share 選單開放。

示意帳號（五個都相同）：student@demo.tw、teacher@demo.tw、admin@demo.tw、outsider@demo.tw（名單外），密碼任意非空。

## 相關設計稿

- Figma（Vedute 規範與各頁設計，進行中）：https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat ；舊入口 wireframe 與提案 A 在 [99] Archive 頁。
- Figma（原 wireframe，封存參考）：https://www.figma.com/design/d2CAvTqsBilnzJkRYPFDgz/socrates-chat-wireframe
- Design 畫布（A 完整流程與 B 三方向入口）：https://claude.ai/artifact/NxprvyyXJ8TY4sAp5jmugj

## 待確認

- Three.js 與 GSAP 都是新增前端主要依賴，依 AGENTS.md 與 S-11 需先經規格討論；這些 prototype 不代表已採用。
- 文案依 2026-11 修訂的 S-02.1：教師可唯讀查看自己教室已結束活動的對話原文；進行中的對話與個人練習看不到。
