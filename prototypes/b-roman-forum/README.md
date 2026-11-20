# Prototype B：羅馬廣場（Forum Romanum）

沉浸式入口提案。三個繪製方向共用同一份廣場幾何、鏡頭站點與登入流程（`forum-core.js`），各自負責材質、渲染與介面：

| 檔案 | 方向 | 繪製方式 | 語氣 |
| --- | --- | --- | --- |
| `vedute.html` | 皮拉內西銅版蝕刻 | 世界座標排線著色＋墨線外殼；版痕框、版內標題，編號圖例即導覽 | 嚴肅思辨 |
| `moneta.html` | 鑄幣廠 | 金屬 PBR＋環境反射；廣場是錢幣坯上的浮雕，外圈銘文 INTERROGARE · NON · RESPONDERE；每輪追問一記錘擊 | 嚴肅、有分量 |
| `salve.html` | 門檻馬賽克 | 全畫面鑲嵌磚後製（錯縫、灰泥、石材色盤量化、邊框色帶）；游標點亮金色玻璃，切站時地磚洗牌，登入是 SALVE 門檻 | 探索 |
| `sticker-v1.html` | v1 貼紙風（未採用，保留作比較） | toon＋白邊外殼 | — |

共同內容：四個站點 I 廣場、II 講壇（對話與想法被打磨）、III 檔案館（誰看得到紀錄）、IV 元老院之門（登入）。只用羅馬建築：檔案館、農神殿、講壇、元老院、尤利亞會堂、卡斯托神殿、灶神殿；蘇格拉底式問答以 Cicero 在拉丁世界的公開詰問為敘事依據，不出現希臘建築或文字。

支援：`prefers-reduced-motion`（不播進場與飛行）、分頁隱藏時停止繪製並暫停所有 tween、手機版（面板貼底、鏡頭拉遠、像素比上限 1.5）、無 WebGL 時只顯示面板。示意帳號：student@demo.tw、teacher@demo.tw、admin@demo.tw、outsider@demo.tw（名單外），密碼任意非空。

每個 HTML 都是單檔，直接用瀏覽器打開即可（需連網載入 CDN 上的 Three.js、GSAP 與字型）。`forum-core.js` 是共用程式的原始檔，已內嵌進三個提案頁：改了它之後執行下列指令重新內嵌（瀏覽器不允許從 `file://` 載入本機模組，所以不用 import）。

```bash
node prototypes/b-roman-forum/inline-core.mjs
```

## 待確認

- Three.js 與 GSAP 已採用（2026-11-20，見 `docs/specs/decisions.md` S-11）。
- 文案依 2026-11 修訂的 S-02.1：教師可唯讀查看自己教室已結束活動的對話原文；進行中的對話與個人練習看不到。
- 連續登入失敗的鎖定策略（S-08.2）尚未呈現。
