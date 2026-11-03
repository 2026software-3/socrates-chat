---
name: 蘇格拉底・教室（Vedute）
description: 把羅馬廣場刻成一張銅版畫；AI 在畫裡只負責追問。
colors:
  paper: "#E8E1CF"
  paper-deep: "#DDD4BE"
  ink: "#1F1A16"
  ink-soft: "#4A4038"
  ink-faint: "rgba(31, 26, 22, .28)"
  placeholder: "#665C4E"
  chalk: "#A9442B"
  chalk-deep: "#8A3520"
  error: "#8A1C12"
typography:
  display-han:
    fontFamily: "Noto Serif TC, Songti TC, serif"
    fontSize: "clamp(30px, 3.6vw, 40px)"
    fontWeight: 900
    lineHeight: 1.3
    letterSpacing: "0.02em"
  headline-han:
    fontFamily: "Noto Serif TC, Songti TC, serif"
    fontSize: "28px"
    fontWeight: 900
    lineHeight: 1.3
    letterSpacing: "0.02em"
  title-han:
    fontFamily: "Noto Serif TC, Songti TC, serif"
    fontSize: "22px"
    fontWeight: 700
    lineHeight: 1.4
  body:
    fontFamily: "Noto Serif TC, Songti TC, serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.7
  note:
    fontFamily: "Noto Serif TC, Songti TC, serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.6
  plate-title:
    fontFamily: "IM Fell English SC, Times New Roman, serif"
    fontSize: "clamp(22px, 2.6vw, 34px)"
    fontWeight: 400
    lineHeight: 1
    letterSpacing: "0.14em"
  latin-label:
    fontFamily: "IM Fell English SC, Times New Roman, serif"
    fontSize: "17px"
    fontWeight: 400
    letterSpacing: "0.08em"
  chalk-note:
    fontFamily: "IM Fell English, Times New Roman, serif"
    fontSize: "15px"
    fontWeight: 400
rounded:
  none: "0px"
  hairline: "2px"
  pin: "50%"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "24px"
  xl: "40px"
  plate-inset: "14px"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.hairline}"
    padding: "0 22px"
    height: "46px"
  button-primary-hover:
    backgroundColor: "{colors.chalk}"
    textColor: "{colors.paper}"
  button-primary-disabled:
    backgroundColor: "{colors.ink-soft}"
    textColor: "{colors.paper}"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.hairline}"
    padding: "0 22px"
    height: "46px"
  link:
    textColor: "{colors.ink}"
    height: "44px"
  input:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "0 2px"
    height: "42px"
  key-station:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    height: "44px"
  key-station-current:
    textColor: "{colors.chalk}"
  pin:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    rounded: "{rounded.pin}"
    size: "34px"
---

# Design System: 蘇格拉底・教室（Vedute）

產品與使用者脈絡見 [PRODUCT.md](PRODUCT.md)。本文件的權威實作是 [`prototypes/b-roman-forum/vedute.html`](prototypes/b-roman-forum/vedute.html)，數值以那份檔案為準；兩者不一致時先修本文件或回報，不要各自分歧。

## Overview

**Creative North Star: "A Plate of the Forum"（一張廣場的銅版畫）**

整個入口是一張 18 世紀 Piranesi 風格的羅馬廣場蝕刻版畫（veduta），印在手工紙上。3D 廣場即時「刻」成排線：光暗決定排線層數，線貼在石頭表面、不隨鏡頭漂移，外輪廓是墨線。介面不是浮在畫上的另一層 UI，而是版畫本身的一部分：說明文字印在版畫天空的留白裡，版畫底部的標題與編號圖例（I 廣場、II 講壇、III 檔案館、IV 元老院之門）就是導覽。

敘事依據是 Cicero：他把蘇格拉底式問答帶進拉丁世界，在羅馬廣場上公開詰問。因此只用羅馬建築（檔案館、農神殿、講壇、元老院、尤利亞會堂、卡斯托神殿、灶神殿）與拉丁銘文，語氣是嚴肅的思辨場所，同時保留想走進去探索的好奇。紅粉筆（sanguine）是唯一的強調色，代表「追問」與「目前所在」。

已確認不要的：像博物館導覽、希臘與羅馬混用、AI 生成感的泛用科技介面。

**Key Characteristics:**
- 單一淺色世界：紙、墨、紅粉筆三種顏色撐起全部畫面，不做深色模式。
- 線而不是面：分隔、欄位、提示都用細墨線，不用填色卡片與陰影。
- 版畫結構即資訊架構：版痕框、版內標題、編號圖例各有功能。
- 拉丁小型大寫字只作標籤與版題，中文永遠是主要閱讀文字。

## Colors

紙色打底、墨色書寫、紅粉筆標記；所有顏色都來自一張紙上的兩種墨。

### Primary
- **Sanguine Chalk 紅粉筆**（#A9442B）：目前所在的站點、AI 的發言標記（INTER.）、hover、焦點框、選取反白、被劃掉的「看不到」項目。只用於標記，不做大面積填色。
- **Deep Sanguine 深紅粉筆**（#8A3520）：紅粉筆需要用在 13px 以下小字或 `paper-deep` 底時的替代色（對比 6.17 / 5.45）。

### Neutral
- **Laid Paper 紙**（#E8E1CF）：頁面底色，加上簾紋、鏈紋與黃斑（見 Elevation）。
- **Deep Paper 深紙**（#DDD4BE）：需要區隔時的次層底色。
- **Etching Ink 蝕刻墨**（#1F1A16）：正文、墨線、主要按鈕底色、穿門的墨水淹沒。對紙 13.2:1。
- **Soft Ink 淡墨**（#4A4038）：次要文字、說明、拉丁副標。對紙 7.7:1。
- **Faint Ink 淡墨線**（rgba(31,26,22,.28)）：版痕框、列表分隔線，只作線不作字。
- **Placeholder**（#665C4E）：輸入框提示字。對紙 5.0:1，對深紙 4.4:1（不足，見 Known Issues）。
- **Error 錯誤**（#8A1C12）：欄位錯誤與登入失敗。對紙 7.1:1。

### Named Rules
**The Two Inks Rule.** 畫面上只有墨與紅粉筆兩種「墨水」。新增狀態色（成功、警告）前，先試著用墨線、劃線、字重表達；真的需要時另立規格，不要直接引入藍綠等外來色。

**The Chalk Is a Mark Rule.** 紅粉筆是手寫在版畫上的記號，不是品牌色塊。單一畫面上紅粉筆面積不超過約 5%。

## Typography

**中文主字體：** Noto Serif TC（fallback Songti TC, serif），權重 400／700／900
**版題與標籤：** IM Fell English SC（fallback Times New Roman, serif）
**手寫註記：** IM Fell English Italic（紅粉筆旁註）

**Character：** 中文用宋體／明體撐起印刷書的正式感；拉丁小型大寫取自 18 世紀英國鉛字，只負責版畫上的銘文與編號，讓畫面有出處而不是裝飾。

### Hierarchy
- **Display**（900，clamp(30px, 3.6vw, 40px)，1.3）：每個站點的主標，一屏一個。手機 28px。
- **Headline**（900，28px，1.3）：站點面板標題，後接拉丁名作副標（例：講壇 Rostra）。手機 24px。
- **Title**（700，22px，1.4）：首頁卡片標題。
- **Body**（400，15px，1.7）：正文，最長約 34em（中文約 34 字一行）。
- **Note**（400，13px，1.6）：法規性說明、示意聲明、圖例。
- **Plate title**（IM Fell English SC，clamp(22px, 2.6vw, 34px)，字距 .14em）：版畫底部的 FORVM ROMANVM。
- **Latin label**（IM Fell English SC，17px，字距 .08em）：拉丁地名、對話標記 DISC.／INTER.、羅馬數字 I–IV。

### Named Rules
**The Latin Is Engraved Rule.** 拉丁文只出現在「版上刻的字」：版題、地名、編號、發言標記。按鈕、錯誤訊息、說明一律用使用者的語言。

**The No Mono Rule.** 不用等寬字體做「技術感」標籤。

## Layout

- **版畫即視窗**：3D 場景鋪滿整個視窗；距邊 14px（含 safe-area）畫一圈版痕框。
- **桌面**：左側一欄面板（寬 min(380px, 100%)），印在版畫天空的留白處，背景是向外淡出的紙色橢圓；鏡頭以 `setViewOffset` 把主體推離面板（panelShift 210px）。底部一列：左為版題，右為編號圖例導覽。
- **手機（< 760px）**：面板改貼底，上緣一條墨線，最高 54vh；版題隱藏，只留圖例；鏡頭再拉遠（phonePush 0.22）。
- **側邊距**：桌面 max(40px, safe-area + 28px)，手機 max(28px, safe-area + 20px)。
- **節奏**：面板內 12px 間距；動作列 10px／18px；首頁區塊 24–26px。
- **首頁（登入後）**：單欄，最寬 760px，置中；以墨線分段，不用卡片。

## Elevation & Depth

完全不用投影。深度只來自三件事：

- **版畫本身的明暗**：3D 場景用排線層數表現光影（見 Components → Engraving）。
- **紙張質感**：底色疊兩層極淡的黃斑（radial-gradient，rgba(150,110,60,.08–.10)）、垂直鏈紋（每 47px 一條，.035）與水平簾紋（每 3px，.018）。
- **版痕框**：一圈 1px 淡墨線，內側一道白色高光與一道墨色暗線，模擬銅版壓進紙裡的凹痕。

### Named Rules
**The Pressed Not Lifted Rule.** 東西是「壓進紙裡」，不是「浮在紙上」。不加 box-shadow 投影、不用毛玻璃、不用浮起的卡片。

## Shapes

- 圓角幾乎為零：按鈕 2px（刀刻的直角），輸入框 0。
- 唯一的圓：版畫上的編號圖釘（34px 圓）、帳號縮寫圈、旋轉中的載入圈。
- 線寬：正文分隔 1px、強調墨線 1.5px、雙線分隔（上下兩條 1px，間距 2px）用在首頁標題下。

## Components

### Buttons
- **Primary**：墨底紙字，46px 高，2px 圓角；hover 換紅粉筆；按下下沉 1px；停用為淡墨且游標為 progress。
- **Quiet**：透明底、1.5px 墨框；hover 時框與字變紅粉筆。
- **Link（ui-link）**：墨字、紅粉筆底線（1.5px，offset 5px），至少 44px 點擊高度；hover 字變紅粉筆。次要動作一律用它，不做第三種按鈕。

### Inputs
- 只有一條 1.5px 底線的「紙上橫線」，透明底、17px 字。
- 焦點：底線變紅粉筆並加 2px 紅粉筆下緣，不加外框。
- 錯誤：底線變錯誤色，下方 13px 錯誤字。
- 標籤 13px 淡墨，在欄位上方。

### Alerts
- 左側一條 1px 細線加 12px 內距，字色決定性質：錯誤用錯誤色，資訊用墨色。不加底色塊。
- 帳密錯誤不透露是哪一欄錯；名單外帳號在登入成功後才顯示「尚未開通」。

### Navigation：版畫圖例（key）
- 羅馬數字（IM Fell English SC 20px）＋中文站名（15px），每項至少 44px 高。
- 目前站點：整項紅粉筆，數字加底線，並設 `aria-current="step"`。
- 版畫上同一個數字以圖釘形式標在對應建築上，點圖釘等同點圖例。

### Dialogue（講壇站）
- 像印刷書裡的對話錄：左欄拉丁發言標記（DISC. 學生、INTER. 提問的 AI，後者紅粉筆），右欄文字；AI 的追問用 700 字重。
- 「想法的刻線」進度條：一條淡墨線上疊 3px 墨線，隨對話輪數由左刻到右。

### Visibility list（檔案館站）
- 看得到的項目前面是墨色勾，看不到的項目用紅粉筆叉並以紅粉筆刪除線劃掉。圖示是自繪 SVG，1.6px 筆畫。
- 文案依 S-02.1：教師可唯讀查看自己教室已結束活動的對話原文；進行中的對話與個人練習看不到；管理者介面訊息一律顯示為 message。

### Engraving（3D 繪製規則）
- 材質：Lambert 打光後算出亮度，暗度 d = clamp(1 − lum × 1.08)。
- 四層排線，依暗度門檻逐層加上（世界座標 triplanar 投影，線不隨鏡頭漂）：
  1. 門檻 0.10，水平面 0°、立面 90°
  2. 門檻 0.38，45°
  3. 門檻 0.58，−45°
  4. 門檻 0.78，與第 1 層垂直
- 線寬以螢幕像素計，不以線距比例計，避免近景出現粗條紋。
- 外輪廓：背面外擴 0.03 的墨色外殼（inverted hull）。
- 物件色只代表「留住多少光」（stone #FFF、brick #BDBDBD、roof #8C8C8C、bronze #707070…），最後只輸出紙與墨兩色。
- 鏡頭站點：I 廣場全景、II 講壇、III 檔案館、IV 元老院之門；站點位置要離開所有幾何體，否則畫面會被柱子塞滿。

### Motion
- **刻版進場**：排線由左到右「刻」出來（uReveal，3.2s，power1.inOut），鏡頭同時從遠處推近（3.6s，power2.out），面板在 2.2s 淡入。提供「略過刻版」。
- **站點移動**：鏡頭 2.0s power2.inOut。
- **穿門**：鏡頭推到元老院門前，雙門向內打開，墨水從中心淹滿全畫面（clip-path 圓，0.6s power2.in），首頁淡入。
- **減少動態**：不播進場、飛行與穿門，直接切換；載入圈停止旋轉。
- 分頁隱藏或離開視窗時停止繪製並暫停所有 tween。

## Do's and Don'ts

**Do**
- 讓每個介面元素都說得出它在一張版畫上的身分（版題、圖例、旁註、刻線）。
- 產品內頁（對話、教室、教師、管理後台）用「克制版」：紙色底、墨線分隔、Noto Serif TC、紅粉筆標記，不放 3D。
- 新圖示使用 Lucide，統一墨色、1.5px 線寬；需要時自繪，筆畫一致。
- 所有互動目標至少 44px，焦點可見（紅粉筆 2px）。

**Don't**
- 不放希臘建築、希臘字母或雅典意象；不寫成博物館解說牌。
- 不用投影、毛玻璃、漸層文字、填色卡片、等寬字標籤。
- 不加第三種強調色；不把紅粉筆當大面積背景。
- 不在管理介面、日誌、報表顯示訊息原文（一律 message）。

## Known Issues

截至 2026-11-03，`vedute.html` 與本規範的已知問題與待辦：

**待確認的規格**
- Three.js 與 GSAP 都是新增前端主要依賴，依 AGENTS.md 與 S-11 須先經規格討論；需開規格 PR 更新 `docs/specs/decisions.md`，含無 WebGL 降級與效能上限。
- 連續登入失敗的鎖定策略（S-08.2）、逾時後是否提示進度保留，尚未呈現。
- 三語（S-11）：IM Fell 只有拉丁字母；英文介面的正文字體與中英混排規則未定。

**可及性與對比**
- 紅粉筆 #A9442B 在 `paper-deep` 上只有 4.0:1，placeholder 在 `paper-deep` 上 4.4:1，都低於 4.5:1；小字或深紙底請改用 `chalk-deep`。
- 面板背景是向外淡出的紙色橢圓，3D 排線很密的區域壓在文字後面時可讀性要再驗證。
- 版畫上的圖釘對螢幕閱讀器是重複的導覽，目前未設 `aria-hidden` 策略，需決定只保留圖例或兩者擇一。

**繪製與效能**
- 手機版面改成面板貼底後尚未重新實機驗證。
- 「廢墟版」場景（倒下的柱鼓、碎石板）容易被讀成考古遺址，與「想進去探索」的語氣有落差；可考慮完整建築版或只保留少量殘件。
- 無 WebGL 時只顯示面板，沒有靜態版畫圖替代；需要一張預先輸出的版畫插圖（也可作社群分享圖與 Figma 參考）。
- 未量測低階手機的幀率與耗電；像素比上限 1.5，尚無自動降級。
- 以 `file://` 開啟需要連網載入 CDN；共用程式由 `inline-core.mjs` 內嵌，改 `forum-core.js` 後要重跑。

**設計範圍缺口**
- 只設計了入口與登入；學生對話介面、教室列表、教師總結與已結束對話、管理後台尚無 Vedute 版本，由組員依本規範的「克制版」在 Figma 繪製。
- Figma `zW14xVQcXF8S5rQuD6YzDM` 的 [00] Guideline 仍是 wireframe 的 `wf/*` 變數與 shadcn 樣式，尚未換成本文件的 token。
