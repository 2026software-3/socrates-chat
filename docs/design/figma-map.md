# Figma 節點對照表與功能地圖

> 本文件由 `node scripts/figma-map/build.mjs` 產生，**不要手改**；要改對照就改 `scripts/figma-map/screens.mjs`，Figma 有新畫面就重新匯出 `scripts/figma-map/frames/`（見 [`scripts/figma-map/README.md`](../../scripts/figma-map/README.md)）。

把 Figma [socrates-chat](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat) [01]–[05] 的每個畫面對到路由、功能目錄、既有測試與負責的 F-41 Issue，讓實作時直接找到節點，不用猜。設計依據的優先順序仍是 `DESIGN.md` → Figma → HTML 原型（PR #653）。

## 怎麼讀

- 每一頁先有**畫面總表**，再有**各畫面的 frame**：一列是一個狀態，三欄是桌機 1440、平板 768、手機 375 的節點；「—」表示該寬度沒畫。
- 路由與功能目錄相對 `frontend/src`；路由來自 `app/routes.tsx` 與 `features/*/index.tsx`、`features/classrooms/routes.tsx`。教師與管理者共用的頁面同時列 `/teacher` 與 `/admin`。
- 狀態：**已重畫**＝Vedute 定稿；**尚未重畫（舊稿）**＝PR #653「明確不包含」的舊畫面，只參考內容、不照版面；**僅提案**＝待需求方確認，不是規範。
- 「尚無路由」＝前端還沒有這個頁面。

## Figma 沒畫的狀態怎麼做

- **載入中一律用 skeleton**，不論 Figma 有沒有畫、畫成什麼樣子（載入圈、點點都不用）。對話頁 AI 回覆前的「正在想下一個問題」掃光是例外，見 #683 與 `DESIGN.md` Dialogue。
- 讀取失敗用 Alert（可重試）；空資料用該頁的空狀態文字，版面沿用已重畫的同頁畫面。
- 只有桌機稿、缺平板或手機的狀態，依同一畫面已重畫的平板／手機版面與 `DESIGN.md` 的斷點規則延伸。

## [01] 入口

Figma 頁面：[8:2](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=8-2)

| 畫面 | 路由 | 功能目錄 | 既有測試 | Issue | 狀態 |
| --- | --- | --- | --- | --- | --- |
| 入口廣場與站點（廣場、講壇、檔案館） | `/login` | `auth` | `auth/password-login.test.tsx` | #690 | 已重畫 |
| 登入（元老院之門）與登入錯誤 | `/login` | `auth` | `auth/password-login.test.tsx` | #690 | 已重畫 |
| 尚未開通 | 任何已登入路由（`Shell` 對名單外帳號顯示 `PendingPage`） | `auth/pending-page.tsx` | — | #690 | 已重畫 |

### 各畫面的 frame

#### 入口廣場與站點（廣場、講壇、檔案館）

3D 場景與轉場數值以 `prototypes/b-roman-forum/vedute.html` 為準，Figma 只作構圖參考。

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 1 廣場 | [2015:267](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-267) | [2249:573](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2249-573) | [2015:279](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-279) | 已重畫 |
| 2 講壇 | [2015:269](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-269) | [2249:600](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2249-600) | [2251:697](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2251-697) | 已重畫 |
| 3 檔案館 | [2015:271](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-271) | [2249:631](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2249-631) | [2251:736](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2251-736) | 已重畫 |

#### 登入（元老院之門）與登入錯誤

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 4 元老院之門 登入 | [2015:273](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-273) | [2249:654](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2249-654) | [2015:281](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-281) | 已重畫 |
| 4 登入錯誤 | [2015:275](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-275) | [2249:680](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2249-680) | [2015:283](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-283) | 已重畫 |

#### 尚未開通

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 4 尚未開通 | [2015:277](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2015-277) | [2249:707](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2249-707) | [2251:767](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2251-767) | 已重畫 |

## [02] 對話

Figma 頁面：[21:240](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=21-240)

| 畫面 | 路由 | 功能目錄 | 既有測試 | Issue | 狀態 |
| --- | --- | --- | --- | --- | --- |
| 對話頁（文字） | `/conversations/:id` | `chat` | `chat/conversation-page.test.tsx`<br>`chat/chat-adapter.test.ts`<br>`chat/sse.test.ts`<br>`chat/argument-tree-panel.test.tsx` | #683 | 已重畫 |
| 對話內語音 | `/conversations/:id` | `chat/voice` | `chat/voice/voice.test.tsx`<br>`chat/voice/sentence-splitter.test.ts` | #683（待確認） | 已重畫 |
| 完成討論與結束確認 | `/conversations/:id` | `chat` | `chat/conversation-page.test.tsx` | #683（待確認） | 已重畫 |
| 結束摘要與推薦書單 | `/conversations/:id` | `chat/summary-panel.tsx` | `chat/conversation-page.test.tsx` | #684 | 已重畫 |
| 結束後唯讀對話（學生） | `/conversations/:id` | `chat` | `chat/conversation-page.test.tsx` | #683（待確認） | 已重畫 |
| 授課教師唯讀對話 | `/teacher/classrooms/:classroomId/conversations/:conversationId` | `classrooms/transcript-page.tsx` | `classrooms/transcript-page.test.tsx` | #687（待確認） | 已重畫 |
| 提案紀錄 · Conversation 動態與視覺提案 | — | — | — | — | 僅提案 |

其他區塊（不是畫面）：說明元件｜圖示旁懸停 [3103:11626](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3103-11626)

### 各畫面的 frame

#### 對話頁（文字）

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 回覆中 | [80:213](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=80-213) | — | — | 已重畫；缺 768、375 |
| 一般對話 | [169:673](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=169-673) | [2296:3572](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2296-3572) | [2296:3594](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2296-3594) | 已重畫 |
| 思考脈絡展開 | [413:19](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=413-19) | [2311:3650](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3650) | [2311:3581](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3581) | 已重畫 |
| 第15回合・收尾階段 | [261:522](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=261-522) | — | — | 已重畫；缺 768、375 |
| 名詞搜尋展開 | [146:340](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=146-340) | [2311:3662](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3662) | [2311:3593](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3593) | 已重畫 |
| 回覆失敗 | [108:288](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=108-288) | — | — | 已重畫；缺 768、375 |

#### 對話內語音

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 麥克風權限・解決方法 | [204:728](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=204-728) | — | — | 已重畫；缺 768、375 |
| 語音辨識失敗 | [158:580](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=158-580) | — | — | 已重畫；缺 768、375 |
| 麥克風權限遭拒 | [169:712](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=169-712) | — | — | 已重畫；缺 768、375 |
| 對話內語音・AI 自動播放 | [2203:5149](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2203-5149) | [2794:6394](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2794-6394) | [2794:6337](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2794-6337) | 已重畫 |
| 對話內語音・聆聽中 | [2203:5215](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2203-5215) | [2311:3674](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3674) | [2311:3605](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3605) | 已重畫 |
| 語音・備援狀態 | [2203:5427](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2203-5427) | — | — | 已重畫；缺 768、375 |
| 語音・回覆中 | [3062:11946](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3062-11946) | — | — | 已重畫；缺 768、375 |
| 對話內語音・轉文字確認 | [3068:13590](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3068-13590) | [3068:12427](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3068-12427) | [3068:11624](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3068-11624) | 已重畫 |
| 語音回覆後・繼續對話 | [3068:15865](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3068-15865) | [3068:15310](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3068-15310) | [3068:14935](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3068-14935) | 已重畫 |

#### 完成討論與結束確認

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 完成討論 | [261:177](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=261-177) | [2332:3650](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2332-3650) | [2332:3572](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2332-3572) | 已重畫 |
| 結束確認 | [261:454](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=261-454) | [2311:3698](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3698) | [2311:3629](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3629) | 已重畫 |

#### 結束摘要與推薦書單

推薦書單前端尚未實作（資料以 PR #658 為準）。

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 推薦書單・備援狀態 | [2203:4943](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2203-4943) | — | — | 已重畫；缺 768、375 |
| 結束摘要與推薦書單 | [2203:5081](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2203-5081) | [2311:3686](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2311-3686) | [3126:7065](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3126-7065) | 已重畫 |

#### 結束後唯讀對話（學生）

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 結束後・唯讀對話 | [261:239](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=261-239) | [2332:3688](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2332-3688) | [2332:3610](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2332-3610) | 已重畫 |
| 結束後・完整思考脈絡 | [3062:12288](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3062-12288) | — | — | 已重畫；缺 768、375 |

#### 授課教師唯讀對話

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 授課教師・唯讀對話 | [2203:5016](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2203-5016) | — | — | 已重畫；缺 768、375 |
| 授課教師・完整思考脈絡 | [3062:12361](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3062-12361) | — | — | 已重畫；缺 768、375 |

#### 提案紀錄 · Conversation 動態與視覺提案

區塊名稱寫明「不是規範」，實作不要照這裡做。

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| 07 名詞查詢與搜尋展開 | [835:1756](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=835-1756) | — | — | 僅提案 |
| 完整畫面預覽 03｜名詞搜尋展開 | [835:1640](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=835-1640) | — | — | 僅提案 |
| 06 關鍵狀態與取捨｜Conversation 動態與視覺提案 | [771:13687](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13687) | — | — | 僅提案 |
| 05 動態分鏡・邏輯圖展開與返回｜Conversation 動態與視覺提案 | [771:13527](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13527) | — | — | 僅提案 |
| 04 動態分鏡・送出到追問｜Conversation 動態與視覺提案 | [771:13406](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13406) | — | — | 僅提案 |
| 03 邏輯圖展開・完整視覺提案｜Conversation 動態與視覺提案 | [771:13303](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13303) | — | — | 僅提案 |
| 02 一般對話・完整視覺提案｜Conversation 動態與視覺提案 | [771:13197](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13197) | — | — | 僅提案 |
| 01 視覺方向：紙與墨，留一盞燈｜Conversation 動態與視覺提案 | [771:13110](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13110) | — | — | 僅提案 |
| 00 封面與導覽｜Conversation 動態與視覺提案 | [771:13039](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=771-13039) | — | — | 僅提案 |
| 完整畫面預覽 01｜一般對話 | [820:557](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=820-557) | — | — | 僅提案 |
| 完整畫面預覽 02｜邏輯圖展開 | [820:634](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=820-634) | — | — | 僅提案 |

## [03] 學生

Figma 頁面：[21:241](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=21-241)。所有畫面都在區塊「正式稿｜跨區連線前版本」（2599:13542）裡。

| 畫面 | 路由 | 功能目錄 | 既有測試 | Issue | 狀態 |
| --- | --- | --- | --- | --- | --- |
| S01 我的教室 | 尚無路由（目前 `/` 依教室分組列出活動） | `student` | `student/available-page.test.tsx` | #681 | 已重畫 |
| S02 教室活動 | `/` | `student/available-page.tsx` | `student/available-page.test.tsx` | #681 | 已重畫 |
| S03 個人練習 | `/` | `student/available-page.tsx` | `student/available-page.test.tsx` | #681 | 已重畫 |
| S04 我的對話 | `/conversations` | `chat/conversation-list-page.tsx` | `chat/conversation-list-page.test.tsx` | #681 | 已重畫 |
| S05 點數與任務 | `/tasks` | `tasks` | `tasks/tasks.test.tsx` | #682 | 已重畫 |
| S06 成就圖鑑 | 尚無路由 | — | — | #682（待確認） | 已重畫 |
| S07 點數紀錄 | 尚無路由 | — | — | #682 | 已重畫 |
| S08 我的蘇格拉底 | 尚無路由 | — | — | #682 | 已重畫 |
| S09 換裝 | 尚無路由 | — | — | #682（待確認） | 已重畫 |
| S10 商店 | `/parts` | `purchase` | `purchase/purchase.test.tsx` | —（待確認） | 已重畫 |

覆蓋層（殼層 #672，不逐一列出）：Overlays · 帳號選單 [2840:28637](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2840-28637)

### 各畫面的 frame

#### S01 我的教室

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S01 我的教室 | [2599:13543](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-13543) | [2599:18522](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18522) | [2599:21240](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21240) | 已重畫 |
| S01 我的教室 · 空狀態 | [2599:16079](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-16079) | [2599:19689](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19689) | [2599:22389](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22389) | 已重畫 |
| S01 我的教室 · 已封存課程 | [2599:16212](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-16212) | [2599:19737](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19737) | [2599:22434](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22434) | 已重畫 |
| S01 我的教室 · 載入中 | [2599:16970](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-16970) | [2599:20387](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20387) | [2599:23080](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23080) | 已重畫 |
| S01 我的教室 · 載入失敗 | [2599:17099](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-17099) | [2599:20431](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20431) | [2599:23121](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23121) | 已重畫 |
| S01 我的教室 · 導覽抽屜 | — | [2599:19595](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19595) | — | 已重畫 |
| S01 我的教室 · 平面圖面板 | — | — | [2599:22295](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22295) | 已重畫 |

#### S02 教室活動

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S02 教室活動 | [2599:15500](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-15500) | [2599:18572](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18572) | [2599:21287](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21287) | 已重畫 |
| S02 已封存課程 · 課程內容 | [2599:16347](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-16347) | [2599:19787](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19787) | [2599:22481](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22481) | 已重畫 |

#### S03 個人練習

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S03 個人練習 | [2599:15370](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-15370) | [2599:18626](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18626) | [2599:21338](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21338) | 已重畫 |
| S03 個人練習 · 導覽抽屜 | — | [2599:20067](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20067) | — | 已重畫 |
| S03 個人練習 · 平面圖面板 | — | — | [2599:22760](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22760) | 已重畫 |

#### S04 我的對話

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S04 我的對話 | [2599:15647](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-15647) | [2599:18676](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18676) | [2599:21386](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21386) | 已重畫 |
| S04 我的對話 · 編輯 | [2599:15789](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-15789) | [2599:18735](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18735) | [2599:21442](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21442) | 已重畫 |
| S04 我的對話 · 確認刪除 | [2599:15930](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-15930) | [2599:18798](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18798) | [2599:21502](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21502) | 已重畫 |
| S04 我的對話 · 導覽抽屜 | — | [2599:20151](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20151) | — | 已重畫 |
| S04 我的對話 · 平面圖面板 | — | — | [2599:22844](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22844) | 已重畫 |

#### S05 點數與任務

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S05 點數與任務 | [2599:13680](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-13680) | [2599:18868](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18868) | [2599:21569](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21569) | 已重畫 |
| S05 點數與任務 · 照片版 | [2599:16492](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-16492) | [2599:19836](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19836) | [2599:22527](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22527) | 已重畫 |
| S05 · 每日已領取 | [2599:17868](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-17868) | [2599:20913](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20913) | [2599:23612](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23612) | 已重畫 |
| S05 · 每週已領取 | [2599:18086](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18086) | [2599:21022](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21022) | [2599:23724](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23724) | 已重畫 |
| S05 · 每日與每週皆已領取 | [2599:18304](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18304) | [2599:21131](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21131) | [2599:23836](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23836) | 已重畫 |
| S05 點數與任務 · 導覽抽屜 | — | [2599:20225](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20225) | — | 已重畫 |
| S05 點數與任務 · 平面圖面板 | — | — | [2599:22918](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22918) | 已重畫 |

#### S06 成就圖鑑

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S06 成就圖鑑 | [2599:13888](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-13888) | [2599:18967](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-18967) | [2599:21671](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21671) | 已重畫 |
| S06 成就圖鑑 · 照片版 | [2599:16710](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-16710) | [2599:19945](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19945) | [2599:22639](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22639) | 已重畫 |

#### S07 點數紀錄

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S07 點數紀錄 | [2599:14130](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-14130) | [2599:19073](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19073) | [2599:21776](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21776) | 已重畫 |

#### S08 我的蘇格拉底

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S08 我的蘇格拉底 | [2599:14285](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-14285) | [2599:19122](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19122) | [2599:21822](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21822) | 已重畫 |
| S08 我的蘇格拉底 · 導覽抽屜 | — | [2599:20305](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20305) | — | 已重畫 |
| S08 我的蘇格拉底 · 平面圖面板 | — | — | [2599:22998](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22998) | 已重畫 |

#### S09 換裝

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S09 換裝 | [2599:14406](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-14406) | [2599:19171](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19171) | [2599:21868](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21868) | 已重畫 |
| S09 換裝 · 造型已儲存 | [2599:14575](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-14575) | [2599:20833](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20833) | [2599:23532](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23532) | 已重畫 |

#### S10 商店

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| S10 商店 | [2599:14742](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-14742) | [2599:19250](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19250) | [2599:21944](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-21944) | 已重畫 |
| S10 商店 · 配件詳情 | [2599:14944](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-14944) | [2599:19357](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19357) | [2599:22052](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22052) | 已重畫 |
| S10 商店 · 試穿 | [2599:15157](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-15157) | [2599:19476](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-19476) | [2599:22172](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-22172) | 已重畫 |
| S10 · 購買成功 | [2599:17229](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-17229) | [2599:20476](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20476) | [2599:23163](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23163) | 已重畫 |
| S10 · 商品詳情・低點數測試 | [2599:17442](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-17442) | [2599:20714](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20714) | [2599:23409](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23409) | 已重畫 |
| S10 · 點數不足 | [2599:17655](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-17655) | [2599:20595](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-20595) | [2599:23286](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2599-23286) | 已重畫 |

## [04] 成果

Figma 頁面：[466:702](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=466-702)

| 畫面 | 路由 | 功能目錄 | 既有測試 | Issue | 狀態 |
| --- | --- | --- | --- | --- | --- |
| R01 教室活動列表 | `/` | `student/available-page.tsx` | `student/available-page.test.tsx` | #681（待確認） | 已重畫 |
| R02 活動詳細 | `/topics/:topicId`（暫對應題目詳情頁） | `student/topic-detail-page.tsx` | `student/topic-detail-page.test.tsx` | #685 | 已重畫，部分狀態尚未重畫 |
| R03 使用指引 | 尚無路由 | — | — | —（待確認） | 已重畫 |
| R04 個人檔案 | `/account` | `account` | `account/account.test.tsx`<br>`dashboard/profile-form.test.tsx`<br>`dashboard/change-password.test.tsx` | #685 | 已重畫 |
| R05 統計資料 | `/account`（個人檔案的頁籤） | `dashboard` | `dashboard/dashboard.test.tsx` | #685 | 已重畫，部分狀態尚未重畫 |
| R06 隱私設定與預覽 | 尚無路由（將是 `/account` 的頁籤） | — | — | #685 | 已重畫，含提案 |
| R07 未儲存離開提醒 | — | — | — | #685 | 僅提案 |
| R08 他人查看的檔案 | 尚無路由 | — | — | —（待確認） | 已重畫 |
| R09 單場對話雷達圖 | `/conversations/:id` | `chat/radar-panel.tsx` | `chat/radar-panel.test.tsx`<br>`chat/radar-chart.test.tsx` | —（待確認） | 已重畫 |
| R10 單場雷達圖 · 維度說明 | `/conversations/:id` | `chat/radar-panel.tsx` | `chat/radar-panel.test.tsx` | —（待確認） | 已重畫 |
| R11 邏輯樹總覽 | `/conversations/:id` | `chat/argument-tree-panel.tsx` | `chat/argument-tree-panel.test.tsx` | —（待確認） | 已重畫，部分狀態尚未重畫 |
| R12 邏輯樹詳情 | `/conversations/:id` | `chat/argument-tree-panel.tsx` | `chat/argument-tree-panel.test.tsx` | —（待確認） | 已重畫 |
| R13–R16 AI 學習總結（含產生中、等待、失敗） | `/conversations/:id`（結束後的總結區塊） | `chat/summary-panel.tsx` | `chat/conversation-page.test.tsx` | #686 | 已重畫，部分狀態尚未重畫 |

覆蓋層（殼層 #672，不逐一列出）：Overlays · 帳號選單 [2840:28772](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2840-28772)、Overlays · 平面圖面板 [2892:10721](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2892-10721)

### 各畫面的 frame

#### R01 教室活動列表

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R01 教室活動列表 | [474:274](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=474-274) | [2116:1779](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1779) | [2116:2002](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2002) | 已重畫 |
| R01 教室活動列表 · 空狀態 | [2116:1629](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1629) | [2116:1891](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1891) | [2116:2093](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2093) | 已重畫 |
| R01 教室活動列表 · 載入中 | [2116:1675](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1675) | [2116:1924](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1924) | [2116:2123](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2123) | 已重畫 |
| R01 教室活動列表 · 讀取失敗 | [2116:1724](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1724) | [2116:1960](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1960) | [2116:2156](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2156) | 已重畫 |
| R01 教室活動列表 · 導覽選單開啟 | — | [2116:8267](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-8267) | — | 已重畫 |
| R01 教室活動列表 · 平面圖面板 | — | — | [2116:8332](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-8332) | 已重畫 |

#### R02 活動詳細

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R02 活動詳細 | [474:364](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=474-364) | [2116:7317](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7317) | [2116:7559](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7559) | 已重畫 |
| R02 活動詳細 · 活動已結束 | [2116:7120](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7120) | [2116:7392](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7392) | [2116:7634](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7634) | 已重畫 |
| R02 活動詳細 · 載入中 | [2116:7180](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7180) | [2116:7442](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7442) | [2116:7684](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7684) | 尚未重畫（舊稿） |
| R02 活動詳細 · 讀取失敗 | [2116:7247](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7247) | [2116:7499](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7499) | [2116:7741](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7741) | 尚未重畫（舊稿） |

#### R03 使用指引

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R03 使用指引 | [474:416](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=474-416) | [2116:1317](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1317) | [2116:1421](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1421) | 已重畫 |

#### R04 個人檔案

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R04 個人檔案 | [474:526](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=474-526) | [2116:2769](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2769) | [2116:3128](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3128) | 已重畫 |
| R04 · 編輯顯示名稱 | [2116:2366](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2366) | [2116:2836](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2836) | [2116:3187](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3187) | 已重畫 |
| R04 · 輸入錯誤 | [2116:2468](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2468) | [2116:2914](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2914) | [2116:3250](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3250) | 已重畫 |
| R04 · 儲存完成 | [2116:2571](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2571) | [2116:2988](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2988) | [2116:3314](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3314) | 已重畫 |
| R04 · 儲存失敗 | [2116:2665](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-2665) | [2116:3053](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3053) | [2116:3371](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3371) | 已重畫 |

#### R05 統計資料

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R05 統計資料 | [475:402](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=475-402) | [2116:3860](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3860) | [2116:3947](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3947) | 已重畫 |
| R05 統計資料 · 空狀態 | [2116:4032](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4032) | [2116:4092](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4092) | [2116:4138](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4138) | 尚未重畫（舊稿） |
| R05 統計資料 · 載入中 | [2116:4184](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4184) | [2116:4268](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4268) | [2116:4334](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4334) | 尚未重畫（舊稿） |
| R05 統計資料 · 讀取失敗 | [2116:4388](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4388) | [2116:4456](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4456) | [2116:4510](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4510) | 尚未重畫（舊稿） |

#### R06 隱私設定與預覽

只做「現況」（顯示名稱、角色外觀可切換，其他鎖定）；「待確認提案」等 S-02.3／S-07.3 決定。

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R06 隱私設定與預覽 | [475:472](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=475-472) | [2116:4958](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4958) | [2116:5060](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5060) | 已重畫 |
| R06 隱私設定與預覽 · 待確認提案 | [2116:5156](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5156) | [2116:5264](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5264) | [2116:5375](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5375) | 僅提案 |

#### R07 未儲存離開提醒

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R07 未儲存離開提醒 · 待確認提案 | [475:556](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=475-556) | [2116:4669](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4669) | [2116:4772](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-4772) | 僅提案 |

#### R08 他人查看的檔案

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R08 他人查看的檔案 | [475:589](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=475-589) | [2116:3543](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3543) | [2116:3649](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3649) | 已重畫 |
| R08 · 未公開狀態 | [2116:3461](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3461) | [2116:3574](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3574) | [2116:3680](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3680) | 已重畫 |
| R08 · 讀取失敗 | [2116:3491](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3491) | [2116:3597](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3597) | [2116:3703](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3703) | 已重畫 |
| R08 · 無權限 | [2116:3517](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3517) | [2116:3623](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3623) | [2116:3729](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-3729) | 已重畫 |

#### R09 單場對話雷達圖

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R09 單場對話雷達圖 | [476:594](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=476-594) | [2116:5805](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5805) | [2116:5892](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5892) | 已重畫 |
| R09 單場對話雷達圖 · 空 | [2116:6002](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6002) | [2116:6024](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6024) | [2116:6046](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6046) | 已重畫 |
| R09 單場對話雷達圖 · 載入中 | [2116:6068](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6068) | [2116:6089](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6089) | [2116:6109](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6109) | 已重畫 |
| R09 單場對話雷達圖 · 讀取失敗 | [2116:6129](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6129) | [2116:6161](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6161) | [2116:6193](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6193) | 已重畫 |

#### R10 單場雷達圖 · 維度說明

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R10 單場雷達圖 · 維度說明 | [476:670](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=476-670) | [2116:5533](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5533) | [2116:5638](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-5638) | 已重畫 |

#### R11 邏輯樹總覽

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R11 邏輯樹總覽 · 原流程對照 | [476:720](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=476-720) | — | — | 尚未重畫（舊稿）；缺 768、375 |
| R11 邏輯樹總覽 · 樹狀版 | [2116:976](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-976) | [2116:1085](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1085) | [2116:1194](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1194) | 已重畫 |
| R11 邏輯樹總覽 · 樹狀版 · 空狀態 | [2116:1027](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1027) | [2116:1136](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1136) | [2116:1243](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1243) | 已重畫 |
| R11 邏輯樹總覽 · 樹狀版 · 載入中 | [2116:1045](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1045) | [2116:1154](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1154) | [2116:1261](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1261) | 已重畫 |
| R11 邏輯樹總覽 · 樹狀版 · 讀取失敗 | [2116:1059](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1059) | [2116:1168](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1168) | [2116:1275](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-1275) | 已重畫 |

#### R12 邏輯樹詳情

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R12 邏輯樹詳情 | [476:779](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=476-779) | [2116:7890](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7890) | [2116:7988](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7988) | 已重畫 |
| R12 邏輯樹詳情 · 首節點 | [2116:7822](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7822) | [2116:7920](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7920) | [2116:8018](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-8018) | 已重畫 |
| R12 邏輯樹詳情 · 末節點 | [2116:7856](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7856) | [2116:7954](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-7954) | [2116:8052](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-8052) | 已重畫 |

#### R13–R16 AI 學習總結（含產生中、等待、失敗）

產生中與等待用 skeleton，失敗用 Alert（#686）。

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| R13 總結產生中 | [477:677](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=477-677) | [2116:8086](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-8086) | [2116:8113](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-8113) | 尚未重畫（舊稿） |
| R14 學習總結 | [477:703](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=477-703) | [2116:6631](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6631) | [2116:6773](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6773) | 已重畫 |
| R15 總結等待較久 | [477:793](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=477-793) | [2116:6390](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6390) | [2116:6438](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6438) | 尚未重畫（舊稿） |
| R16 總結產生失敗 | [477:826](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=477-826) | [2116:6282](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6282) | [2116:6319](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6319) | 尚未重畫（舊稿） |
| R14 學習總結 · 讀取失敗 | [2116:6915](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6915) | [2116:6948](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6948) | [2116:6981](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2116-6981) | 已重畫 |

## [05] 教師與管理者

Figma 頁面：[21:243](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=21-243)。「· 管理者」結尾的列在區塊「管理者工作區 · 桌面／平板／手機」，是同一畫面的管理者版本（路由前綴 `/admin`）。

| 畫面 | 路由 | 功能目錄 | 既有測試 | Issue | 狀態 |
| --- | --- | --- | --- | --- | --- |
| E01 教室列表 | `/teacher/classrooms`、`/admin/classrooms` | `classrooms/classrooms-page.tsx` | `classrooms/classrooms-page.test.tsx` | #687 | 已重畫 |
| E02 新增教室 | `/teacher/classrooms`、`/admin/classrooms`（對話框） | `classrooms/classrooms-page.tsx` | `classrooms/classrooms-page.test.tsx` | #687 | 已重畫 |
| E03 教室詳細頁 | `/teacher/classrooms/:classroomId`、`/admin/classrooms/:classroomId` | `classrooms/classroom-pages.tsx` | `classrooms/classroom-pages.test.tsx` | #687 | 已重畫 |
| E04 匯入學生名單與結果 | `/teacher/roster`、`/admin/roster` | `teacher/roster-page.tsx` | `teacher/roster-page.test.tsx` | #687（待確認） | 已重畫 |
| E05 封存教室 | `/teacher/classrooms`、`/admin/classrooms`（對話框） | `classrooms/classrooms-page.tsx` | `classrooms/classrooms-page.test.tsx` | #687 | 已重畫 |
| E06 已封存教室 | `/teacher/classrooms/:classroomId`、`/admin/classrooms/:classroomId` | `classrooms/classroom-pages.tsx` | `classrooms/classroom-pages.test.tsx` | #687 | 尚未重畫（舊稿） |
| E11–E13 班級論點分布、主張與理由、重新分組 | `/teacher/classrooms/:classroomId/activities/:activityId`、`/admin/classrooms/:classroomId/activities/:activityId` | `classrooms/classroom-pages.tsx` | `classrooms/classroom-pages.test.tsx` | #687 | 已重畫 |
| E14 管理者班級參與概況 | `/admin/classrooms/:classroomId` | `classrooms/classroom-pages.tsx` | `classrooms/classroom-pages.test.tsx` | #689 | 已重畫 |
| E21 學生總結列表 | `/teacher/summaries` | `teacher/summaries-page.tsx` | `teacher/summaries-page.test.tsx` | #689 | 已重畫 |
| E22 學生總結詳細內容 | `/teacher/classrooms/:classroomId/students/:studentId`、`/admin/classrooms/:classroomId/students/:studentId` | `classrooms/classroom-pages.tsx` | `classrooms/classroom-pages.test.tsx` | #689 | 已重畫 |
| E23 已結束對話原文（唯讀） | `/teacher/classrooms/:classroomId/conversations/:conversationId`、`/admin/classrooms/:classroomId/conversations/:conversationId`（管理者版每則訊息以 `message` 顯示（S-02.1）） | `classrooms/transcript-page.tsx` | `classrooms/transcript-page.test.tsx` | #689 | 已重畫 |
| E31–E35 討論活動（列表、新增／編輯、發布、結束、重新開放） | `/teacher/classrooms/:classroomId`、`/admin/classrooms/:classroomId`（教室頁的討論活動頁籤） | `teacher/activities-page.tsx` | `teacher/activities-page.test.tsx` | #687 | 已重畫 |
| E36 已結束活動 · 學生參與名單 | `/teacher/classrooms/:classroomId/activities/:activityId`、`/admin/classrooms/:classroomId/activities/:activityId` | `classrooms/classroom-pages.tsx` | `classrooms/classroom-pages.test.tsx` | #687（待確認） | 尚未重畫（舊稿） |
| E41–E44 題目庫、新增／編輯、停用、啟用 | `/teacher/topics`、`/admin/topics` | `admin/topics-page.tsx` | `admin/topics-page.test.tsx`<br>`teacher/topic-bank.test.tsx` | #688 | 已重畫 |
| E51–E55 AI 題目建議 | 尚無路由 | — | — | #688（待確認） | 尚未重畫（舊稿） |
| E61–E65 共用書庫與書目 | `/teacher/books`、`/admin/books` | `books` | `books/books.test.tsx`<br>`books/work-actions.test.tsx`<br>`books/related-editions.test.tsx`<br>`books/book-language.test.tsx`<br>`books/book-works.test.ts`<br>`books/book-review-fixes.test.tsx` | #688 | 已重畫 |
| E66–E67 題目預設書單、活動內本題書單 | `/teacher/topics`、`/admin/topics`、`/teacher/classrooms/:classroomId/activities/:activityId`、`/admin/classrooms/:classroomId/activities/:activityId`（對話框） | `books/book-list.tsx` | `books/book-lists.test.tsx` | #688 | 已重畫 |
| E71–E77 學生帳號（列表、移出、刪除、停用、復原、重設密碼） | `/teacher/roster`、`/admin/roster` | `teacher/roster-page.tsx` | `teacher/roster-page.test.tsx`<br>`teacher/account-management.test.tsx` | #689 | 已重畫 |
| E81 管理者總覽 | `/admin` | `admin/admin-home.tsx` | `admin/admin-home.test.tsx` | #689 | 已重畫 |
| E82–E85 教師管理 | `/admin/teachers` | `admin/teachers-page.tsx` | `admin/teachers-page.test.tsx` | —（待確認） | 尚未重畫（舊稿） |
| E86 部件價格設定 | `/admin/parts` | `purchase/admin-parts-page.tsx` | `purchase/purchase.test.tsx` | —（待確認） | 尚未重畫（舊稿） |
| E87 任務數值設定 | `/admin/tasks` | `tasks/admin-tasks-page.tsx` | `tasks/tasks.test.tsx` | —（待確認） | 尚未重畫（舊稿） |

覆蓋層（殼層 #672，不逐一列出）：Overlays · 帳號選單 [2840:29008](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2840-29008)、Overlays · 平面圖面板 [2891:17481](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2891-17481)

### 各畫面的 frame

#### E01 教室列表

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E01 教室列表 | [94:278](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=94-278) | [2220:2729](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-2729) | [2225:4656](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-4656) | 已重畫 |
| E01 教室列表 · 側欄展開 | — | [2220:11691](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-11691) | — | 已重畫 |
| E01 教室列表 · 平面圖面板 | — | — | [2225:10235](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-10235) | 已重畫 |
| E01 教室列表 · 已封存 | — | [3391:32350](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3391-32350) | [3391:32438](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3391-32438) | 已重畫；缺 1440 |
| E01 教室列表 · 管理者 | [3397:60218](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60218) | [3397:37304](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37304) | [3397:50987](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-50987) | 已重畫 |
| E01 教室列表 · 已封存 · 管理者 | — | [3397:39771](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39771) | [3397:53456](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53456) | 已重畫；缺 1440 |

#### E02 新增教室

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E02 新增教室 | [107:342](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=107-342) | [2220:2819](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-2819) | [2225:4833](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-4833) | 已重畫 |
| E02 Error | [170:330](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=170-330) | [2220:2924](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-2924) | [2225:4735](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-4735) | 已重畫 |
| E02 新增教室 · 管理者 | [3397:61009](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61009) | [3397:37392](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37392) | [3397:51205](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51205) | 已重畫 |
| E02 Error · 管理者 | [3397:61359](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61359) | [3397:37520](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37520) | [3397:51076](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51076) | 已重畫 |

#### E03 教室詳細頁

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E03 教室詳細頁 | [177:432](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=177-432) | [2220:3034](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3034) | [2225:4931](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-4931) | 已重畫 |
| E03 教室詳細頁 · 管理者 | [3397:60313](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60313) | [3397:37649](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37649) | [3397:51333](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51333) | 已重畫 |

#### E04 匯入學生名單與結果

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E04-2 匯入結果與臨時密碼 | [177:642](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=177-642) | [2220:3219](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3219) | [2225:5100](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5100) | 已重畫 |
| E04-1 匯入學生名單 | [177:565](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=177-565) | [2220:3121](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3121) | [2225:5009](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5009) | 已重畫 |
| E04-1 匯入學生名單 · 管理者 | [3397:60861](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60861) | [3397:37739](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37739) | [3397:51424](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51424) | 已重畫 |
| E04-2 匯入結果與臨時密碼 · 管理者 | [3397:60617](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60617) | [3397:37836](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37836) | [3397:51523](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51523) | 已重畫 |

#### E05 封存教室

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E05 封存教室 | [177:828](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=177-828) | [2220:3348](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3348) | [2225:5220](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5220) | 已重畫 |
| E05 封存教室 · 管理者 | [3397:61138](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61138) | [3397:37965](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-37965) | [3397:51676](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51676) | 已重畫 |

#### E06 已封存教室

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E06 已封存教室 | [177:994](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=177-994) | [2220:3454](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3454) | [2225:5317](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5317) | 尚未重畫（舊稿） |
| E06 已封存教室 · 管理者 | [3397:60497](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60497) | [3397:38090](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-38090) | [3397:51803](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51803) | 尚未重畫（舊稿） |

#### E11–E13 班級論點分布、主張與理由、重新分組

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E11 班級論點分布 | [206:1086](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=206-1086) | [2220:3508](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3508) | [2225:5361](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5361) | 已重畫 |
| E12 各題目主張與理由展開 | [233:1160](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=233-1160) | [2220:3702](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3702) | [2225:5533](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5533) | 已重畫 |
| E13 重新分組確認 | [252:275](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-275) | [2220:3823](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3823) | [2225:5646](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5646) | 已重畫 |

#### E14 管理者班級參與概況

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E14 管理者班級參與概況 | [252:604](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-604) | [2220:3967](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-3967) | [2225:5782](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5782) | 已重畫 |

#### E21 學生總結列表

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E21 學生總結列表 | [252:773](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-773) | [2220:4045](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4045) | [2225:5849](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-5849) | 已重畫 |

#### E22 學生總結詳細內容

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E22 學生總結詳細內容 | [252:891](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-891) | [2220:4223](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4223) | [2225:6018](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6018) | 已重畫 |

#### E23 已結束對話原文（唯讀）

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E23 已結束對話原文 · 唯讀 | [2206:2623](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2206-2623) | [2206:2675](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2206-2675) | [2206:2727](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2206-2727) | 已重畫 |

#### E31–E35 討論活動（列表、新增／編輯、發布、結束、重新開放）

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E31 討論活動列表 | [252:1435](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-1435) | [2220:4306](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4306) | [2225:6091](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6091) | 已重畫 |
| E35 重新開放確認 | [252:1498](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-1498) | [2220:4892](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4892) | [2225:6606](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6606) | 已重畫 |
| E33 發布確認 | [252:1536](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-1536) | [2220:4561](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4561) | [2225:6295](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6295) | 已重畫 |
| E32 新增／編輯討論活動 | [252:1645](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-1645) | [2220:4457](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4457) | [2225:6232](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6232) | 已重畫 |
| E34 結束活動確認 | [252:1723](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=252-1723) | [2220:4727](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-4727) | [2225:6451](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6451) | 已重畫 |
| E31 討論活動列表 · 管理者 | [3397:60401](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60401) | [3397:38119](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-38119) | [3397:51832](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-51832) | 已重畫 |
| E32 新增／編輯討論活動 · 管理者 | [3397:60957](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60957) | [3397:38331](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-38331) | [3397:52045](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52045) | 已重畫 |
| E33 發布確認 · 管理者 | [3397:60765](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60765) | [3397:38363](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-38363) | [3397:52078](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52078) | 已重畫 |
| E34 結束活動確認 · 管理者 | [3397:61265](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61265) | [3397:38586](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-38586) | [3397:52310](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52310) | 已重畫 |
| E35 重新開放確認 · 管理者 | [3397:60523](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-60523) | [3397:38807](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-38807) | [3397:52540](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52540) | 已重畫 |

#### E36 已結束活動 · 學生參與名單

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E36 已結束活動 · 學生參與名單 | [2206:3067](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2206-3067) | [2220:5056](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5056) | [2225:6760](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6760) | 尚未重畫（舊稿） |

#### E41–E44 題目庫、新增／編輯、停用、啟用

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E44 啟用題目確認 | [290:9194](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=290-9194) | [2220:5609](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5609) | [2225:7273](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7273) | 已重畫 |
| E43 停用題目確認 | [290:8954](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=290-8954) | [2220:5392](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5392) | [2225:7065](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7065) | 已重畫 |
| E42 新增／編輯題目 | [290:8837](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=290-8837) | [2220:5291](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5291) | [2225:6974](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6974) | 已重畫 |
| E41 題目庫列表 | [290:8621](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=290-8621) | [2220:5099](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5099) | [2225:6793](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-6793) | 已重畫 |
| E41 題目庫列表 · 管理者 | [3397:59990](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-59990) | [3397:39028](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39028) | [3397:52770](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52770) | 已重畫 |
| E42 新增／編輯題目 · 管理者 | [3397:59941](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-59941) | [3397:39139](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39139) | [3397:52882](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52882) | 已重畫 |
| E43 停用題目確認 · 管理者 | [3397:59705](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-59705) | [3397:39189](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39189) | [3397:52939](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-52939) | 已重畫 |
| E44 啟用題目確認 · 管理者 | [3397:59468](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-59468) | [3397:39321](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39321) | [3397:53074](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53074) | 已重畫 |

#### E51–E55 AI 題目建議

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E55 產生失敗（保留已輸入內容） | [320:3773](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=320-3773) | [2220:6889](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-6889) | [2225:7926](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7926) | 尚未重畫（舊稿） |
| E54 捨棄／確認採用 | [320:3859](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=320-3859) | [2220:6136](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-6136) | [2225:7760](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7760) | 尚未重畫（舊稿） |
| E53 編輯建議題目 | [320:4008](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=320-4008) | [2220:6039](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-6039) | [2225:7673](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7673) | 尚未重畫（舊稿） |
| E52 AI 產生結果（查看建議題目） | [320:4091](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=320-4091) | [2220:5890](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5890) | [2225:7534](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7534) | 尚未重畫（舊稿） |
| E51 AI 題目建議（輸入主題與需求） | [320:4217](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=320-4217) | [2220:5826](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-5826) | [2225:7480](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-7480) | 尚未重畫（舊稿） |
| E51 AI 題目建議（輸入主題與需求） · 管理者 | [3397:61784](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61784) | [3397:39453](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39453) | [3397:53209](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53209) | 尚未重畫（舊稿） |
| E52 AI 產生結果（查看建議題目） · 管理者 | [3397:61700](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61700) | [3397:39490](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39490) | [3397:53247](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53247) | 尚未重畫（舊稿） |
| E53 編輯建議題目 · 管理者 | [3397:61648](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61648) | [3397:39575](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39575) | [3397:53294](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53294) | 尚未重畫（舊稿） |
| E54 捨棄／確認採用 · 管理者 | [3397:61547](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61547) | [3397:39628](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39628) | [3397:53353](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53353) | 尚未重畫（舊稿） |
| E55 產生失敗（保留已輸入內容） · 管理者 | [3397:61490](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61490) | [3397:39728](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39728) | [3397:53412](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53412) | 尚未重畫（舊稿） |

#### E61–E65 共用書庫與書目

依 S-06.3、PR #652（2026-11-21）。

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E61 共用書庫 | [2928:19218](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2928-19218) | [2933:20590](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2933-20590) | [2933:20849](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2933-20849) | 已重畫 |
| E62 新增書目 ① 找書 | [2930:19359](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2930-19359) | [3392:32593](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-32593) | [2988:24739](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2988-24739) | 已重畫 |
| E62-2 新增書目 ③ 其他版本 | [2930:19538](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2930-19538) | [3392:32921](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-32921) | [2988:25057](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2988-25057) | 已重畫 |
| E63 編輯書目（自己新增的版本） | [2931:19660](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2931-19660) | [3392:33070](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-33070) | [2988:25242](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2988-25242) | 已重畫 |
| E64 整理作品歸組 | [2931:19812](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2931-19812) | [3392:33232](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-33232) | [2988:25357](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2988-25357) | 已重畫 |
| E65 共用書庫（管理者） | [2931:19949](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2931-19949) | [3392:32372](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-32372) | [2989:25139](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2989-25139) | 已重畫 |
| E65-2 永久刪除書目（管理者） | [2931:20253](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2931-20253) | [3392:33371](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-33371) | [2988:25479](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2988-25479) | 已重畫 |
| E62-1 新增書目 ② 確認資料 | [2977:21853](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2977-21853) | [3392:32747](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3392-32747) | [2988:24935](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2988-24935) | 已重畫 |
| E63 編輯書目（管理者） | — | [3397:58339](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-58339) | [3397:58648](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-58648) | 已重畫；缺 1440 |

#### E66–E67 題目預設書單、活動內本題書單

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E66 題目預設書單 | [2932:20304](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2932-20304) | [3393:36934](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3393-36934) | [2989:25869](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2989-25869) | 已重畫 |
| E67 活動內本題書單 | [2932:20705](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2932-20705) | [3393:37136](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3393-37136) | [2989:26366](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2989-26366) | 已重畫 |
| E66 題目預設書單 · 管理者 | [3397:61820](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61820) | [3397:39859](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-39859) | [3397:53545](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53545) | 已重畫 |
| E67 活動內本題書單 · 管理者 | [3397:61926](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-61926) | [3397:40049](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-40049) | [3397:53654](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3397-53654) | 已重畫 |

#### E71–E77 學生帳號（列表、移出、刪除、停用、復原、重設密碼）

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E71 學生帳號列表 | [332:357](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-357) | [2220:7349](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-7349) | [2225:8316](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-8316) | 已重畫 |
| E72 移出教室確認 | [332:523](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-523) | [2220:7465](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-7465) | [2225:8422](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-8422) | 已重畫 |
| E73 刪除帳號確認 | [332:715](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-715) | [2220:7610](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-7610) | [2225:8557](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-8557) | 已重畫 |
| E74 停用帳號確認（僅管理者） | [332:907](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-907) | [2220:7756](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-7756) | [2225:8693](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-8693) | 已重畫 |
| E75 復原帳號確認（僅管理者） | [332:1096](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-1096) | [2220:7901](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-7901) | [2225:8824](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-8824) | 已重畫 |
| E76 重設密碼確認 | [332:1284](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-1284) | [2220:8046](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8046) | [2225:8955](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-8955) | 已重畫 |
| E77 重設密碼結果 | [332:1472](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=332-1472) | [2220:8187](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8187) | [2225:9086](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9086) | 已重畫 |
| E71 學生帳號列表（管理者） | — | [3395:37667](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-37667) | [3395:37876](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-37876) | 已重畫；缺 1440 |
| Overlay · 管理者學生帳號 · 停用帳號 | — | [3395:37805](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-37805) | [3395:38034](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-38034) | 已重畫；缺 1440 |
| Overlay · 管理者學生帳號 · 復原帳號 | — | [3395:37823](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-37823) | [3395:38054](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-38054) | 已重畫；缺 1440 |
| Overlay · 管理者學生帳號 · 重設密碼 | — | [3395:37841](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-37841) | [3395:38074](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-38074) | 已重畫；缺 1440 |
| Overlay · 管理者學生帳號 · 重設密碼結果 | — | [3395:37859](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-37859) | [3395:38094](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3395-38094) | 已重畫；缺 1440 |
| Overlay · 管理者學生帳號 · 刪除帳號 | — | [3396:37235](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3396-37235) | [3396:37284](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3396-37284) | 已重畫；缺 1440 |
| Overlay · 管理者學生帳號 · 移出教室 | — | [3396:37257](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3396-37257) | [3396:37308](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=3396-37308) | 已重畫；缺 1440 |

#### E81 管理者總覽

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E81 管理者總覽 | [338:381](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-381) | [2220:8324](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8324) | [2225:9213](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9213) | 已重畫 |
| E81 管理者總覽 · 側欄展開 | — | [2220:11710](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-11710) | — | 已重畫 |
| E81 管理者總覽 · 平面圖面板 | — | — | [2225:10327](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-10327) | 已重畫 |

#### E82–E85 教師管理

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E82 教師管理列表 | [338:474](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-474) | [2220:8369](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8369) | [2225:9244](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9244) | 尚未重畫（舊稿） |
| E83 教師帳號詳細／編輯 | [338:618](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-618) | [2220:8480](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8480) | [2225:9342](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9342) | 尚未重畫（舊稿） |
| E84 重設教師密碼 | [338:730](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-730) | [2220:8544](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8544) | [2225:9392](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9392) | 尚未重畫（舊稿） |
| E85 重設密碼結果 | [338:888](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-888) | [2220:8672](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8672) | [2225:9507](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9507) | 尚未重畫（舊稿） |

#### E86 部件價格設定

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E86 部件價格設定 | [338:1044](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-1044) | [2220:8797](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-8797) | [2225:9835](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9835) | 尚未重畫（舊稿） |
| E86-2 部件設定變更確認 | [482:1567](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=482-1567) | [2220:9073](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-9073) | [2225:9619](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-9619) | 尚未重畫（舊稿） |

#### E87 任務數值設定

| 狀態 | 1440 | 768 | 375 | 狀態 |
| --- | --- | --- | --- | --- |
| E87 任務數值設定 | [338:1164](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=338-1164) | [2220:9000](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2220-9000) | [2225:10025](https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=2225-10025) | 尚未重畫（舊稿） |

## 尚未重畫的狀態（彙整）

實作時只參考內容；載入中用 skeleton，失敗用 Alert，版面沿用同頁已重畫的畫面。

- [04] 成果 R02 活動詳細 · 載入中（#685）
- [04] 成果 R02 活動詳細 · 讀取失敗（#685）
- [04] 成果 R05 統計資料 · 空狀態（#685）
- [04] 成果 R05 統計資料 · 載入中（#685）
- [04] 成果 R05 統計資料 · 讀取失敗（#685）
- [04] 成果 R11 邏輯樹總覽 · 原流程對照（—）
- [04] 成果 R13 總結產生中（#686）
- [04] 成果 R15 總結等待較久（#686）
- [04] 成果 R16 總結產生失敗（#686）
- [05] 教師與管理者 E06 已封存教室（#687）
- [05] 教師與管理者 E06 已封存教室 · 管理者（#687）
- [05] 教師與管理者 E36 已結束活動 · 學生參與名單（#687）
- [05] 教師與管理者 E55 產生失敗（保留已輸入內容）（#688）
- [05] 教師與管理者 E54 捨棄／確認採用（#688）
- [05] 教師與管理者 E53 編輯建議題目（#688）
- [05] 教師與管理者 E52 AI 產生結果（查看建議題目）（#688）
- [05] 教師與管理者 E51 AI 題目建議（輸入主題與需求）（#688）
- [05] 教師與管理者 E51 AI 題目建議（輸入主題與需求） · 管理者（#688）
- [05] 教師與管理者 E52 AI 產生結果（查看建議題目） · 管理者（#688）
- [05] 教師與管理者 E53 編輯建議題目 · 管理者（#688）
- [05] 教師與管理者 E54 捨棄／確認採用 · 管理者（#688）
- [05] 教師與管理者 E55 產生失敗（保留已輸入內容） · 管理者（#688）
- [05] 教師與管理者 E82 教師管理列表（—）
- [05] 教師與管理者 E83 教師帳號詳細／編輯（—）
- [05] 教師與管理者 E84 重設教師密碼（—）
- [05] 教師與管理者 E85 重設密碼結果（—）
- [05] 教師與管理者 E86 部件價格設定（—）
- [05] 教師與管理者 E86-2 部件設定變更確認（—）
- [05] 教師與管理者 E87 任務數值設定（—）
- [04] 成果 R02 活動詳細 · 空狀態（#685）：Figma 沒有畫

## 待確認：路由或負責 Issue 不明確

- [02] 對話 對話內語音：#683 只寫「輸入列與送出」，沒有點名語音狀態（聆聽中、轉文字確認、AI 自動播放、麥克風權限、備援）。
- [02] 對話 完成討論與結束確認：結束流程的彈窗沒有寫在 #683 或 #684，暫歸 #683（同一頁）。
- [02] 對話 結束後唯讀對話（學生）：#683、#684 都沒有點名「結束後・唯讀對話」，暫歸 #683。
- [02] 對話 授課教師唯讀對話：#687 寫「教師唯讀檢視」、#689 寫「對話原文」（[05] E23），兩張都可能負責，需決定一張。
- [03] 學生 S06 成就圖鑑：#682 只寫「點數與任務（手機任務與成就兩欄）」，獨立的成就圖鑑頁沒有負責 Issue。
- [03] 學生 S09 換裝：#682 的「我的蘇格拉底（外觀切換、配戴）」可能包含換裝頁，需確認。
- [03] 學生 S10 商店：商店（現在的「兌換部件」`/parts`）沒有列在 #681、#682，沒有負責 Issue。
- [04] 成果 R01 教室活動列表：與 [03] S02 教室活動重疊；#685 沒有點名 R01，暫歸 #681，需確認以哪一份為準。
- [04] 成果 R03 使用指引：使用指引沒有負責 Issue，也還沒有路由。
- [04] 成果 R08 他人查看的檔案：他人查看的檔案（F-20 分享）沒有負責 Issue，也還沒有路由。
- [04] 成果 R09 單場對話雷達圖：雷達圖沒有列在任何 F-41 頁面 Issue（#683–#686）。
- [04] 成果 R10 單場雷達圖 · 維度說明：同 R09，沒有負責 Issue。
- [04] 成果 R11 邏輯樹總覽：邏輯樹沒有列在任何 F-41 頁面 Issue；對話頁的思考脈絡（[02]）屬 #683，成果區的樹狀版待決定。
- [04] 成果 R12 邏輯樹詳情：同 R11，沒有負責 Issue。
- [05] 教師與管理者 E04 匯入學生名單與結果：#687 寫「教室三頁籤：學生名單」，#689 寫「學生帳號」；匯入流程歸哪一張需確認。
- [05] 教師與管理者 E36 已結束活動 · 學生參與名單：尚未重畫，#687 沒有點名，暫歸 #687。
- [05] 教師與管理者 E51–E55 AI 題目建議：尚未重畫；F-05 AI 題目建議也還沒實作，#688 只列為相關。
- [05] 教師與管理者 E82–E85 教師管理：尚未重畫，沒有負責 Issue。
- [05] 教師與管理者 E86 部件價格設定：尚未重畫，沒有負責 Issue。
- [05] 教師與管理者 E87 任務數值設定：尚未重畫，沒有負責 Issue。

## 沒有 Figma 畫面的路由

沿用 Vedute 元件與殼層（#670–#672）改版，不另畫。

| 路由 | 檔案 | 用途 |
| --- | --- | --- |
| `/complaint` | `complaints/complaint-page.tsx` | 客訴（S-17） |
| `/admin/complaints` | `complaints/admin-complaints-page.tsx` | 管理者客訴列表 |
| `/admin/services` | `admin/services-page.tsx` | 服務健康度（S-09.3） |
| `/admin/diagnostics` | `admin/diagnostics-page.tsx` | 診斷 |
| `/admin/announcements` | `announcements/admin-announcements-page.tsx` | 公告與維護模式（S-15） |
| `/admin/system-ai` | `keys/system-ai-page.tsx` | 系統 AI 設定 |
| `/admin/teachers/keys` | `keys/admin-keys-page.tsx` | 教師金鑰 |
| `/teacher/keys` | `keys/keys-page.tsx` | 教師金鑰 |
| `/teacher/classrooms/:classroomId/ai` | `keys/ai-settings-page.tsx` | 教室 AI 設定 |
| `/teacher/classrooms/:classroomId/students` | `classrooms/classroom-pages.tsx` | 教室學生名單（Figma 併在 E03 頁籤） |
| `/admin/classrooms/:classroomId/students` | `classrooms/classroom-pages.tsx` | 教室學生名單（管理者） |
| `/dashboard` | `dashboard/index.tsx` | 舊網址，導向 `/account` |
| （任何路由） | `announcements/maintenance-page.tsx` | 維護模式 |
| （任何路由） | `auth/disabled-page.tsx` | 帳號停用 |
| （任何路由） | `auth/change-password-page.tsx` | 臨時密碼須先改密碼 |
