# 視覺原型 prototypes

Vedute（廣場版畫）視覺方向的 HTML 原型，給前端實作時參考畫面與動畫。資料、帳號都是示意，跟正式前端程式無關。視覺規範見根目錄 [DESIGN.md](../DESIGN.md)，各頁設計稿在 [Figma](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat)（從 [00] Guideline 開始讀）。

## 內容

| 檔案 | 內容 |
| --- | --- |
| [b-roman-forum/vedute.html](b-roman-forum/vedute.html) | **入口**：可以轉動的 3D 廣場版畫（排線著色、墨線輪廓、版痕框）。編號圖例就是導覽，I 廣場、II 講壇、III 檔案館、IV 元老院之門依序移動鏡頭；登入在元老院之門，成功後穿門、墨水淹滿畫面再進入內頁。 |
| [c-interior/index.html](c-interior/index.html) | **系統內頁（登入後）**：Topbar＋廣場平面圖側欄＋版畫頁首。換頁時平面圖上的建築打開、Topbar 的拉丁名逐字刻出；手機改成底部分頁列＋平面圖面板。可以切換學生、教師與對話頁。細節見 [c-interior/README.md](c-interior/README.md)。 |

其他入口提案（`a-plaza/`、`b-roman-forum/` 的 sticker-v1、moneta、salve）沒有採用，只留作紀錄，不再維護。

## 怎麼開

內頁會載入同資料夾的 css、js 與圖片，要用本機伺服器開：

```bash
python3 -m http.server 4173 --directory prototypes
```

然後打開 http://localhost:4173/b-roman-forum/vedute.html ，登入後會進到內頁；也可以直接開 http://localhost:4173/c-interior/index.html 。需要連網，才能載入 CDN 上的函式庫與字型。

示意帳號：student@demo.tw、teacher@demo.tw、admin@demo.tw、outsider@demo.tw（名單外），密碼隨意輸入、不能空白。

## 注意

- 入口原型用了 Three.js（3D 繪製）與 GSAP（動畫）。這兩個函式庫還沒有被正式前端採用：依 AGENTS.md，新增主要的前端依賴要先經過規格討論（S-11）。實作入口前要先決定要不要引入，或改用其他做法。
- 文案依 S-02.1：教師可以唯讀查看自己教室已結束活動的對話原文；進行中的對話與個人練習看不到。
