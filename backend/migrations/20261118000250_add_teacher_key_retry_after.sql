-- F-40.5 (#599): 供應商以 Retry-After 要求的等待秒數；受頻率限制的 key 依此冷卻（至少 60 秒、最多 1 小時，由應用層限制）。
-- 只往前：新增可為 NULL 的欄位，不改動既有資料。
ALTER TABLE teacher_api_keys
    ADD COLUMN IF NOT EXISTS retry_after_secs integer CHECK (retry_after_secs > 0);
