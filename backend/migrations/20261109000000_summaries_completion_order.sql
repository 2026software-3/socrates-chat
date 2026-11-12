-- #418：總結列表（GET /api/summaries）改依總結完成時間（completed_at DESC, conversation_id）
-- 做 keyset 分頁，並要求 ready 的總結一定有 completed_at。
--
-- #418 原本直接改了 20261002040000 的 CHECK 與 summaries_ready_idx，但那個檔案已套用在 NTNU
-- （2026-11-07 起視為正式環境），已合併的 migration 不修改（S-09.3、CONTRIBUTING.md）。
-- 舊檔已還原成原本內容，同樣的 schema 變化改由這個往前的 migration 完成；
-- 全新的資料庫與已上線的資料庫套完後的 schema 相同。

-- 防禦性補值：程式寫入 ready 時一律同時寫 completed_at，正常情況沒有資料需要補。
UPDATE summaries SET completed_at = started_at WHERE status = 'ready' AND completed_at IS NULL;

ALTER TABLE summaries DROP CONSTRAINT summaries_check;
ALTER TABLE summaries ADD CONSTRAINT summaries_check
    CHECK (status <> 'ready' OR (stance IS NOT NULL AND reasons IS NOT NULL AND completed_at IS NOT NULL));

-- 總結列表依 completed_at DESC, conversation_id 做 keyset 分頁，只列 ready。
DROP INDEX summaries_ready_idx;
CREATE INDEX summaries_ready_idx ON summaries (completed_at DESC, conversation_id) WHERE status = 'ready';
