# API 盤點、規劃與設計 audit

> 狀態（2026-10）：3.2、3.7、3.8 已處理；3.1、3.3、3.4、3.5 已各拆成以單一接口為單位的 issue（#284–#294）；登入鎖定加 IP 為 #295。其餘 3.6、3.9 的細項仍待排。
>
> 範圍：`backend/src` 的路由（以 `.route(` 為準）對照 `docs/specs/` 與 open issues。
> 「未實作」一節的路徑是**提案**，不是規格；實作前要依 CONTRIBUTING 走規格決策流程。
> 已實作 API 的逐項說明見 [`api-reference.md`](api-reference.md)。

## 1. 已實作（45 個 operation、38 條路徑）

| 領域 | 路由 | 權限 |
| --- | --- | --- |
| 健康 | `GET /health` | 公開 |
| 登入 | `GET /api/auth/google/login`、`GET /api/auth/google/callback`、`POST /api/auth/login`、`POST /api/auth/change-password`、`POST /api/auth/logout`、`GET /api/me` | 公開／登入 |
| 管理者 | `GET·POST /api/admin/teachers`、`DELETE /api/admin/teachers/{email}`、`POST /api/admin/users/reset-password`、`POST /api/admin/users/{email}/disable`、`…/enable` | admin |
| 題庫 | `GET·POST /api/admin/topics`、`PATCH /api/admin/topics/{id}`；`GET·POST /api/topics`、`PATCH /api/topics/{id}`（同功能兩組路徑） | admin／teacher |
| 活動 | `GET·POST /api/activities`、`GET·PATCH /api/activities/{id}`、`POST …/publish`、`POST …/close`、`GET /api/available` | teacher／student |
| 名單 | `GET /api/roster`、`POST /api/roster/import`、`DELETE /api/roster/{email}`、`GET /api/students`、`POST /api/students/{email}/delete` | teacher |
| 對話 | `GET·POST /api/conversations`、`GET·DELETE /api/conversations/{id}`、`POST …/messages`、`GET …/stream`（SSE）、`POST …/end` | student |
| 總結 | `GET …/summary`、`POST …/summary/retry`、`GET /api/teacher/summaries` | student／teacher |
| 儀表板 | `GET /api/dashboard/me`、`GET /api/dashboard/class`、`POST /api/dashboard/class/regroup` | student／teacher |
| 語音 | `POST /api/voice/transcribe`、`POST /api/voice/speech` | student |

## 2. 未實作（依 open issues 與規格整理，提案）

| Issue | 功能 | 建議路由 | 備註 |
| --- | --- | --- | --- |
| F-19.1/19.2/19.4、S-07.2 | 點數餘額與帳本 | `GET /api/points`（餘額）、`GET /api/points/ledger?cursor=` | 僅本人；發點在後端事件（總結 ready）內完成，不開放寫入 API |
| F-19.3、S-07.4 | 任務與領取 | `GET /api/tasks`（含進度、週期）、`POST /api/tasks/{id}/claim` | 領取冪等：重複領取要明確回 409，與規格「重複遭拒絕」一致 |
| F-17.1–17.3 | 部件、購買、外觀 | `GET /api/parts`、`POST /api/parts/{id}/unlock`、`GET·PUT /api/me/avatar` | 與兌換合併成同一購買流程（S-07 §1） |
| F-19.4 | 管理員定價 | `PATCH /api/admin/parts/{id}`、`PATCH /api/admin/tasks/{id}` | 須寫稽核日誌 |
| F-18 | 成就 | `GET /api/achievements` | 解鎖由後端事件觸發，無寫入 API；S-07.3 未定 |
| F-20 / S-07.6 | 成果分享 | `POST /api/shares`、`GET /api/shares/{token}` | 預設隱藏個資；格式未定 |
| F-12 | 邏輯樹 | `GET /api/conversations/{id}/logic-tree` | S-04.4 未定 |
| F-13 | 思辨標籤 | `GET /api/conversations/{id}/labels` | S-04.5 未定 |
| F-14 | 閱讀推薦、教師書單 | `GET /api/conversations/{id}/recommendations`、`GET·POST·DELETE /api/books` | S-06.3 未定 |
| F-10.3 | 雙方逐字稿同步 | 現有 `messages.source` 已部分涵蓋；是否需新端點待 F-10.3 釐清 | |
| F-28 | 教室（跨年度） | `/api/classrooms`、`/api/classrooms/{id}/roster`… | 與「系統只服務一門課、沒有班級」的前提衝突，需先改規格 |
| F-27 / S-13 | 名詞解釋 | `GET /api/glossary?lang=` | 來源未定 |
| S-09 | 營運探針 | `GET /ready`（含 DB 檢查） | technical-details 已提到，目前只有 `/health` |

## 3. 已實作部分的 audit

嚴重度：**高**＝可能造成錯誤或資安問題；**中**＝一致性／可維護性；**低**＝建議。

### 做得好的

- 錯誤格式統一、只回 `code` + `request_id`，不洩漏內部資訊（`error.rs`）。
- 非擁有者存取對話一律 404，不洩漏存在與否（`chat.rs` `owned_conversation`）。
- CSRF 用 Origin 比對 + `SameSite=Lax` + HttpOnly cookie，符合同網域部署。
- SSE 用資料庫 `generating_since` 原子 claim 防止雙串流，且斷線後仍存完整回覆。
- PATCH 採 merge-patch 語意，有獨立測試（`patch_null.rs`）。
- 管理者遮蔽（`message`）在輸出層集中處理。

### 3.1【高】沒有分頁／上限（issue：#284 對話、#285 總結、#286 學生、#287 名單、#288 活動）

`GET /api/conversations`、`/api/teacher/summaries`、`/api/students`、`/api/roster`、`/api/activities` 全部 `fetch_all`。`teacher/summaries` 還 JOIN 三表且隨全班歷史成長，會是第一個變慢的端點。
**建議**：統一 `?limit=&cursor=`（keyset，依 `ended_at, id`），回 `{ items, next_cursor }`。教師端再加 `?student=`、`?topic=` 篩選。因為還沒上線，現在改成本最低。

### 3.2【高】`POST /api/conversations/{id}/messages` 與 `/stream` 分兩步（**已處理：採方案 b**）

流程是「POST 存訊息 → 再 GET SSE」。兩步之間若前端斷線或忘了開串流，對話停在「最後一則是學生訊息」（靠 `nothing_to_reply` 與 stale claim 補救）；而且 `send_message` 已把 `turn_count` +1，之後開串流失敗也不會回退。
**可選方案**：(a) 維持現狀，但文件明列「必須接著開串流」並補孤兒訊息的重試語意（目前 retry＝重開串流，已可行）；(b) 改成 `POST …/messages` 直接回 SSE（assistant-ui 的 transport 多半是這種），省一次往返。S-08.4 已定案兩步式，改動需先改規格；建議至少做 (a)。

### 3.3【中】動作式路徑與動詞誤用（issue：#289、#290、#291）

- `POST /api/students/{email}/delete` 應為 `DELETE /api/students/{email}`（需要 `confirm_email` body 的話，可用 `DELETE` + body，或改為 `?confirm=`／`X-Confirm-Email` header；若顧慮 DELETE body 相容性，現狀可接受但要在文件註明是刻意）。
- `POST /api/admin/users/{email}/disable|enable` 可收斂成 `PUT /api/admin/users/{email}/status` `{ disabled: bool, reason? }`，冪等且只有一條路由；`end`／`publish`／`close` 這類狀態轉換用動作式 POST 是常見務實做法，可保留，但同一組資源請擇一風格。
- 資源識別用 email 放在 path：停用／復原帳號的對象一定有帳號，改用 user id（UUID，#290）；名單與教師項目可能沒有帳號，維持 email。目前後端沒有掛 request logging（無 `TraceLayer`），路徑不會進日誌；日後加上時要過濾路徑中的 email。
- `GET /api/available` 名稱不明確，建議 `GET /api/catalog` 或 `GET /api/conversation-sources`。

### 3.4【中】同一資源兩組路徑（issue：#292、#293、#294）

`/api/admin/topics` 與 `/api/topics` 行為相同（`catalog.rs` 兩組 handler）。**建議**只留 `/api/topics`，用角色決定能做什麼；admin 前綴留給「只有管理者」的功能。少一倍測試與文件維護。

### 3.5【中】角色與資源的混合命名（issue：#285 `/api/summaries`）

`/api/teacher/summaries`、`/api/dashboard/class`、`/api/roster`（教師用）、`/api/students`（教師用）混用「以角色命名」與「以資源命名」。學生的 summary 是 `/conversations/{id}/summary`，教師的是 `/teacher/summaries`。
**建議**：以資源命名，權限由後端判斷，例如 `GET /api/summaries`（教師全部／學生自己的）；或維持角色前綴但全面一致（`/api/teacher/*`、`/api/admin/*`、`/api/student/*`）。目前兩者混用，前端與文件都要記例外。

### 3.6【中】`/api/me` 與 `must_change_password` 的白名單

臨時密碼旗標由 session 層擋掉其他 API（403 `password_change_required`），這設計合理。風險是新增路由時若忘了走同一個 extractor 就會漏擋；`auth.rs` 若是在 extractor 內統一檢查，建議補一個「遍歷所有路由、確認在旗標下皆為 403」的整合測試，以後新增 F-17/F-19 端點時自動保護。

### 3.7【中】文件與實作漂移（**已處理**）

- `S-08.1-component-dataflow.md` 原本寫 `/api/speech/transcribe`、`/api/speech/tts?message_id=`；實作是 `/api/voice/transcribe`、`/api/voice/speech`（body 帶 `text`）。規格已改成實作的路徑。是否要 `message_id` 版 TTS（避免前端回傳全文、可快取）仍可日後評估。
- `api-reference.md` 的教師總結欄位列了 `turning_points`、`position`，但 `teacher_list` 的 SQL 只取 `stance, reasons, claim, framework, framework_scores`；文件裡 summary 說明又同時出現 `claim`。已對照 `TeacherRow`／`SummaryView` 更正為 `claim`、`framework_scores`。
- `error.rs` 的 `ApiError.message` 欄位不會輸出（S-12.1 只回 code），但每個呼叫點仍填中文訊息，容易誤導。可移除欄位，或僅在日誌使用。

### 3.8【中】沒有 API 契約的機器可讀來源（**已處理：utoipa**）

45 個 operation（38 條路徑）全靠手寫 Markdown，已出現 3.7 的漂移。**建議**引入 OpenAPI（`utoipa`）或至少在整合測試中固定回應 shape；新增依賴需先經規格討論（AGENTS.md）。

### 3.9【低】細節

- `POST /api/conversations/{id}/end` 回 202（首次）與 200（已結束）：狀態碼雙義，對前端是額外分支。建議一律 202 或一律 200 + body `{ status }`。
- `POST /api/conversations` 的 `invalid_source`（活動／題目不可用）與 `send_message` 的 `invalid_source`（`source` 欄位非法）同名不同義。`send_message` 的碼與欄位名一致，應保留；建議改 `create` 的碼：`invalid_conversation_source`，或拆成 `activity_unavailable`／`topic_unavailable`。前端只依賴 `create` 的碼（`common.messages.ts` 三語翻譯與 `available-page.test.tsx`），`send_message` 的碼前端沒有翻譯。
- `POST /api/roster/import` 一次回傳明文臨時密碼：已限制只出現一次、僅管理者，OK；但回應建議加 `Cache-Control: no-store`。
- 登入鎖定 5 次／15 分鐘是以信箱為單位，可被用來鎖住他人帳號；加 IP 維度見 #295（會改 S-08.2，需先更新規格）。
- 沒看到全域 request body 大小限制與請求逾時（`tower_http` 只用在靜態檔）。語音有 10 MB 上限，其餘靠 axum 預設 2 MB；建議明確設定並加 `TimeoutLayer`（SSE 路由除外）。
- `/health` 之外沒有 `/ready`（見第 2 節）。
- 管理者操作的稽核只有 disable／enable／delete；reset-password 與日後的調價（S-07 §5 要求）也要進 `audit_logs`，請確認 reset-password 已寫入。

## 4. 建議的處理順序

1. 分頁與上限（3.1）、`/ready`、body 限制：趁未上線一次做。
2. 路徑整理（3.3、3.4、3.5）：先定命名規則寫進 `docs/specs/`，再批次改；前端目前仍在開發中，成本最低的時間點是現在。
3. 修正文件漂移（3.7），並決定 3.8 是否採 OpenAPI。
4. 新功能（第 2 節）依 S-07 → F-19.1 → F-19.2/19.3 → F-17 的相依順序，沿用「事件內入帳 + UNIQUE 冪等」設計，不開放前端寫入點數。
