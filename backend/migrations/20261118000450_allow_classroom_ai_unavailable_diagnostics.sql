-- 教室的 AI 不可用（F-40.5）也要記成 AI 診斷事件，管理者才看得到。原本的 CHECK 只允許七種 AI 失敗分類，
-- 寫入 classroom_ai_unavailable 會被拒絕（只留一行 "diagnostic write failed" 警告）。
-- 原本的約束沒有命名，依定義找出來刪掉，再以固定名稱重建。
DO $$
DECLARE
    old_name text;
BEGIN
    SELECT conname INTO STRICT old_name
      FROM pg_constraint
     WHERE conrelid = 'diagnostic_events'::regclass
       AND contype = 'c'
       AND pg_get_constraintdef(oid) LIKE '%invalid_output%';
    EXECUTE format('ALTER TABLE diagnostic_events DROP CONSTRAINT %I', old_name);
END
$$;

ALTER TABLE diagnostic_events ADD CONSTRAINT diagnostic_events_failure_class_allowed CHECK (
    failure_class IS NULL OR
    (kind = 'ai' AND failure_class IN ('connection_failed', 'timeout', 'invalid_key',
        'quota_exceeded', 'rate_limited', 'service_error', 'invalid_output',
        'classroom_ai_unavailable')) OR
    (kind = 'login' AND failure_class IN ('invalid_credentials', 'account_disabled', 'account_locked')) OR
    (kind = 'account_lock' AND failure_class = 'account_locked')
);
