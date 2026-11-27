# S-20 AI 工作背景執行與全域進度顯示

2026-11-26，需求方指示：AI 功能通常要等一段時間，應在背景運行，不卡住前端，不因前端切換頁面而中斷，並讓使用者清楚知道工作正在跑。本文件為此決策的詳細規格；摘要見 [`decisions.md`](decisions.md) S-20。

## 1. 原則

1. **使用者觸發、非即時互動、可能超過數秒的 AI 呼叫，一律背景化**：請求只負責「登記工作」並立即回 `202`；AI 呼叫在後端 `tokio::spawn` 內完成，結果寫入資料庫。請求斷線、前端切頁、重新整理都不影響工作。
2. **登記工作先 commit 再 spawn**（沿用 `radar_store` 的 claim 做法）：不會出現請求被取消而工作遺失或重複扣額度。
3. **前端不把工作綁在元件生命週期**：進行中的工作由全域追蹤器持有，切頁照樣輪詢；重新整理後由伺服器回報進行中的工作。
4. **一定看得到**：只要有工作在跑，任何頁面都能看到進度指示；完成與失敗都主動通知。
5. 不新增依賴、不新增基礎設施（不引入佇列或 Redis）。

## 2. 範圍盤點

| 功能 | 現況 | 本規格要求 |
| --- | --- | --- |
| 對話回覆（SSE） | 已背景產生，可接回 | 不改傳輸；回到頁面自動接回（S-08.1、S-08.4）。納入全域指示（對話進行中顯示「AI 回覆中」） |
| 對話總結、單場雷達、論證樹 | 已背景化，輪詢綁元件 | 後端不改；前端改走全域追蹤器 |
| 總結多語翻譯（worker） | 已背景化 | 納入全域指示（`translation_pending`） |
| 教師重新分組 | 請求內同步等待 | 改為背景工作（`ai_jobs`，kind=`regroup`） |
| 教師看總結的翻譯 | 請求內同步等待 | 改為背景工作（kind=`summary_translation_view`） |
| 題目維度產生與重試 | 記憶體互斥、無狀態欄位 | 改為有狀態（`ai_jobs`，kind=`topic_dimensions`），並可輪詢 |
| 教室 AI 設定儲存前驗證模型 | 同步驗證才儲存 | 改為背景工作（kind=`classroom_ai_verify`）：先登記驗證，通過才套用設定；失敗不套用 |
| 成就翻譯 | 請求內同步 | 儲存成就後背景補譯（kind=`achievement_translation`），沿用公告翻譯「失敗標 pending、背景重試」做法 |
| 金鑰測試、模型清單（教師與管理者） | 同步，受單次逾時限制 | **維持同步**（單次探測、有逾時、結果立即決定下一步）；前端加 `AbortController` 並在離頁時不顯示過期結果 |
| 語音辨識、語音合成、即時語音 | 即時互動 | **不背景化**（本質上需要即時回應，離開頁面即結束是預期行為） |
| 公告翻譯、題目標題翻譯 | 已有背景重試 | 不改 |
| 主張分組（assign_group） | 在總結任務內盡力而為 | 不改 |

## 3. 後端

### 3.1 `ai_jobs` 表（新 migration，只新增）

給上表中「新背景化」的功能共用，不取代 `summaries`、`conversation_radar_results`、`conversation_argument_trees`。

| 欄位 | 說明 |
| --- | --- |
| `id` | uuid，主鍵 |
| `kind` | 文字，上表列出的 kind |
| `owner_user_id` | 發起者；只有發起者（與管理者的遮蔽版）能查 |
| `scope_key` | 同一範圍的互斥鍵，例如 `regroup:{classroom_id}`；對未結束的工作設唯一索引，避免重複啟動 |
| `status` | `pending` / `running` / `ready` / `failed` |
| `attempt` | 嘗試代次，條件式更新防止過期任務寫回 |
| `result` | jsonb，已驗證的結果摘要；**不保存學生訊息、AI 總結、逐字稿或原始 AI 輸出** |
| `error_code` | 固定代碼，不含堆疊、金鑰或供應商診斷 |
| `created_at` / `updated_at` / `finished_at` | 時間 |

- 登記：同一交易內檢查權限、扣 AI 額度（`charge_ai`）、寫入 `pending`，commit 後才 spawn。已有同 `scope_key` 未結束的工作時，回傳那個工作（`200`，不重複扣額度）。
- 失敗或取消：退回額度（`refund_ai`），`status=failed`，帶固定 `error_code`。
- 單一工作的時間上限沿用該功能原本的逾時設定。
- 啟動恢復：把所有逾時仍是 `pending`／`running` 的 `ai_jobs` 標為 `failed`（`error_code=interrupted`），行為比照 `summary::recover_orphaned`；使用者可重試。
- 保留：`ready`／`failed` 的記錄保留 7 天後清除，不影響各功能自己的結果表。
- 多程序限制與現況相同：互斥靠資料庫唯一索引，不靠程序記憶體。

### 3.2 各端點契約

- 原本同步的端點改為 `202 Accepted`，回 `{ job_id, status }`；`GET /api/ai-jobs/{id}` 查單一工作（只有發起者，他人與不存在一律 404）。
- 重試：`POST` 同一端點，僅 `failed` 可重試，會新增嘗試代次。
- 教室 AI 設定：`PUT /api/classrooms/{id}/ai-settings` 回 `202`，驗證通過才寫入設定；`GET` 設定仍回目前生效的設定，另附 `pending_job`。
- 封存教室：不可啟動新工作（`409 classroom_archived`），規則與 S-14 一致。
- 新增端點都要有 `#[utoipa::path]`，並更新 `docs/api/api-reference.md`。

### 3.3 彙總端點

`GET /api/ai-jobs/active`：回傳目前使用者所有進行中，以及最近 10 分鐘內剛結束的 AI 工作，供全域進度顯示使用。內容為統一的精簡結構：

```json
{ "jobs": [ { "id": "...", "kind": "summary", "status": "pending",
              "label_key": "jobs.summary", "scope": { "conversation_id": "..." },
              "started_at": "...", "finished_at": null, "retryable": false } ] }
```

- 來源：`ai_jobs`，加上 `summaries`、`conversation_radar_results`、`conversation_argument_trees` 的 `pending`／最近結束列，以及 `generating=true` 的對話。
- 只回發起者自己的工作；角色不符、名單外帳號與他人資料的存取都要有測試。
- `scope` 只帶識別碼，不帶題目文字、訊息或總結內容；`label_key` 是 i18n 鍵，由前端翻譯。
- 管理者看不到他人的工作（管理介面沒有此需求）。

## 4. 前端

### 4.1 全域追蹤器

- 在 `app/layout.tsx` 的 provider 層新增 `AiJobsProvider`，所有工作的輪詢都在此持有，不在頁面元件內。切頁、關閉對話框都不中止輪詢。
- 先實作 `src/lib/use-polling`（`docs/vedute-frontend-plan.md` 的 T-5）：可設定間隔與上限、分頁隱藏時降頻、在 unmount 時清理。總結、雷達、論證樹三個 hook 改用它，保留既有間隔與上限。
- 重新整理或重新登入後，追蹤器以 `GET /api/ai-jobs/active` 還原進行中的工作；有進行中工作時以 2 秒輪詢，沒有時不輪詢，只在觸發新工作時喚醒。
- 各功能頁面仍可訂閱自己的工作以更新畫面（例如總結面板），資料來源是追蹤器，不自己 `fetch`。

### 4.2 進度指示（使用者一定看得到）

- **全域指示**：頁首固定顯示「AI 工作中（N）」的小膠囊，有進行中工作時出現，帶旋轉指示與計數；點開列出每個工作的名稱、所屬對話或教室、已經過時間與狀態，並提供連到來源頁面的連結。沒有工作時不顯示。
- **就地指示**：發起工作的畫面（總結面板、雷達面板、論證樹、重新分組按鈕、教室 AI 設定、題目維度）維持 skeleton 或「處理中」文字，並顯示「可離開此頁，完成後會通知」。
- **完成通知**：工作結束時，若使用者已不在來源頁面，用 toast 通知「已完成／失敗」，附連結；失敗可直接重試。若仍在來源頁面則只更新畫面，不重複跳 toast。
- 預估時間過長時（例如論證樹超過 15 分鐘）顯示「比平常久，仍在處理」，不假裝失敗。
- 無障礙：進度膠囊與 toast 使用 `aria-live="polite"`，狀態不能只靠顏色；axe 不得有 serious、critical 違規（S-11.6）。
- 文字走 i18n（繁中、英文、西文），顏色與字級只用 token 與 Vedute 元件；視覺與版面細節依 `DESIGN.md`＋Figma（此處尚未有節點，見 §6）。

### 4.3 同步呼叫的保護

仍保持同步的金鑰測試與模型清單：加 `AbortController`，離頁時中止；回應晚於離頁不得更新畫面或跳通知。

## 5. 資安與個資

- `ai_jobs.result`、`error_code`、彙總端點與日誌都**不得**帶訊息內容、AI 總結、學生反思或供應商診斷；日誌只記工作 id、kind、狀態、request id（沿用 `diagnostics::inherit_request`）。
- 授權：未登入 401、角色不符 403、名單外帳號 403、他人的工作 404；每條都要有測試。
- 測試資料只用合成資料。

## 6. 尚待確認

以下不得自行決定，實作時在 PR 中標示，向 @whyhugo 確認：

- 進度膠囊與工作清單的視覺、位置、RWD 行為（需要 Figma 節點與 `DESIGN.md` 補充）。
- 「最近結束」顯示的時間窗（暫定 10 分鐘）與 `ai_jobs` 保留天數（暫定 7 天）。
- 教室 AI 設定改為「先驗證再套用」後，儲存按鈕與目前生效設定之間的文案。

## 7. 實作切分（每項各一個 PR）

1. **S-20 規格**（本文件與 `decisions.md`）。
2. **前端基礎**：`use-polling`、`AiJobsProvider`，三個既有輪詢 hook 改用。尚無全域 UI。
3. **後端彙總**：`GET /api/ai-jobs/active`（先只彙總現有的總結、雷達、論證樹、回覆中）。
4. **前端全域進度**：膠囊、工作清單、完成 toast、就地提示（依 §6 確認的設計）。
5. **後端 `ai_jobs` 表與恢復機制**，加第一個使用者：教師重新分組。
6. **教師看總結的翻譯**改背景。
7. **題目維度**改為有狀態並輪詢。
8. **教室 AI 設定**改為背景驗證後套用。
9. **成就翻譯**改背景補譯。
10. **金鑰測試**前端離頁保護。

每個 PR 都依 TDD 先寫失敗測試；前端 PR 依 AGENTS.md 驗收清單（Playwright、三種寬度截圖、axe、lint）。
