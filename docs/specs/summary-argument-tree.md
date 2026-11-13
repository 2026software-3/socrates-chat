# 按需摘要論證樹

2026-11-10 本次需求：在現有前端提供「總結論證樹」按鈕；學生查看自己的對話；結果保存，重新整理後可讀取。

此功能沿用 main_argument_tree 的單一父節點摘要樹。它與 S-04.4 多對多論證圖是不同的資料契約，不據此關閉 S-04.4／F-12，也不改寫 Revision 定義。

## 使用流程與範圍

- 按鈕位於學生自己的已結束對話頁、AI 學習總結下方。進行中的對話不提供按鈕，後端也拒絕生成與讀取。
- 打開時先讀取已保存結果；只有尚無結果時才要求生成。依完整已保存對話分析，不以既有總結代替原文，不接受前端上傳任意逐字稿。
- 樹與原總結分別產生、分別保存，原總結失敗不阻止按需產樹。
- 第一次生成採背景工作；前端每秒查詢，關閉視窗或離頁時中止輪詢，背景工作繼續。
- 成功結果直接重用，不提供重新生成或手動編輯；失敗可明確點擊重試。重新整理後再打開按鈕，讀取保存結果，不另付一次模型呼叫。
- 顯示可展開的論點層級、種類、關係、分析輪次與遮蔽後引文，也可查看 tree JSON。AI 分析標示尚未人工複核。介面支援繁中、英文、西文；節點文字依對話語言產生。

## API 與資料

- GET /api/conversations/{id}/argument-tree：200，回傳 status、tree、prompt_version、completed_at。
- status 為 missing／pending／ready／failed；tree 只有 ready 時有值，為既有 DisplayTree JSON（topic_id、title、node_count、review_status、root；節點含 children）。
- POST 同一路徑：無需 request body；202 表示產生中，200 表示重用成功結果。資料庫主鍵與條件式更新保證只啟動一個工作，不重複扣額度。
- 使用既有 sid cookie 與 CSRF 規則。未登入 401、名單外或角色不符 403；他人的或不存在的對話一律 404。管理者不得讀取分析結果；教師不提供此功能。
- 進行中對話回 409 conversation_not_ended；沒有學生發言回 409 empty_conversation；無法轉接的逐字稿回 409 argument_tree_input_invalid。
- 封存教室內只可讀取既有結果，不可啟動新生成或重試（409 classroom_archived），沿用教室唯讀規則。
- 生成計入既有 AI 用量；超額 429 ai_usage_exceeded，失敗或未啟動的重複請求退回額度。GET 與重用成功結果不計額度。
- 新增 conversation_argument_trees，主鍵 conversation_id，保存狀態、嘗試代次、已驗證 tree JSON、提示版本、實際模型與時間。不保存未驗證原始輸出或內部錯誤。
- 刪除對話時，樹經外鍵 ON DELETE CASCADE 一併刪除。migration 只新增，不修改既有 migration。

## 生成與失敗處理

- 透過 AiProvider 的結構化呼叫使用 Responses API、strict JSON Schema、store=false，沿用 OPENAI_MODEL、OPENAI_BASE_URL、金鑰池、備援模型與 OPENAI_REASONING_EFFORT。服務與模型必須支援 Responses 及結構化輸出，不自行降級。
- CLI 的 ANNOTATION_* 仍僅作用於 CLI，不覆寫服務端設定。
- 沿用 v7 解析、連續學生訊息合併、逐欄遮蔽、輸入白名單、結構與逐字引文驗證。來源訊息位置不傳模型；分析輪次不是資料庫回合數。
- API 節點 ID 使用結果內的 TREE 前綴；對話識別由授權 URL 與資料庫外鍵維持，不把使用者或對話識別資料送給模型。
- 拒答、部分回應、逾時、非法 JSON、結構或引文錯誤均為 failed，不保存無效結果，可重試。錯誤、日誌不帶逐字稿或供應商診斷。
- 單次產生的時間上限是 `ARGUMENT_TREE_TIMEOUT_SECS`（預設 300 秒），與一般 AI 回覆的 `AI_TOTAL_TIMEOUT_SECS` 分開：推理強度維持 `OPENAI_REASONING_EFFORT`（high），在合成逐字稿（5／10／15 輪）上單次呼叫實測 36–208 秒（14 次，其中 7 次超過 120 秒、沒有超過 300 秒），曾在正式環境因 120 秒上限而失敗。把強度降到 medium 可縮短到 12–74 秒，但開場立場較常被省略（B、C 兩份較長的逐字稿中，第 1 輪保留 4/11 對 6/10），所以選擇保留 high、放寬逾時。
- 失敗會寫一筆 `diagnostic_events`（`function=argument_tree`，`failure_class` 為 timeout／供應商錯誤分類／invalid_output，`error_code` 區分 `argument_tree_input_failed`、`argument_tree_ai_failed`、`argument_tree_validation_failed`）並以 `tracing` 記錄相同的分類；兩者都不含逐字稿或模型輸出。
- 工作只能更新自己的嘗試代次。超過 `ARGUMENT_TREE_TIMEOUT_SECS` 再加 30 秒仍 pending 的孤兒工作，下次讀取標為 failed，可重試；舊工作不得覆蓋新代次。
- 瀏覽器只接收 JSON，由 React 當成文字與元件呈現，不插入模型 HTML；回應 no-store。

## Given / When / Then

1. Given 學生的已結束對話，When 首次點按鈕，Then 背景生成並保存已驗證樹，顯示視覺化與 JSON。
2. Given 已成功生成，When 重新整理後打開，Then 讀取相同保存結果，不呼叫模型。
3. Given 同時多次點擊，When 請求到達，Then 只啟動一次生成；其他請求取得 pending 或已保存結果。
4. Given 模型失敗或輸出不合法，When 工作結束，Then failed、退回額度、不回傳原始輸出，可重試。
5. Given 未登入、角色不符、名單外或他人對話，When 讀取或生成，Then 拒絕且不呼叫模型。
6. Given 新代次已啟動，When 舊代次較晚完成，Then 不覆蓋新狀態。
7. Given 刪除對話，When 刪除成功，Then 不留下樹資料。

尚不包含 S-04.4 正式圖譜、每輪更新、語意去重驗證、教師顯示與人工編輯。
