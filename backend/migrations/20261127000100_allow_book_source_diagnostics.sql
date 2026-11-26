-- 外部書目查詢（Google Books／Open Library）失敗時也要寫入 diagnostic_events，讓管理者在後台看得到。
-- 原本的未命名 kind CHECK 只允許 'ai'、'login'、'account_lock'，diagnostic_events_failure_class_allowed 也未包含 book_source。
DO $$
DECLARE
    kind_constraint text;
BEGIN
    SELECT conname INTO STRICT kind_constraint
      FROM pg_constraint
     WHERE conrelid = 'diagnostic_events'::regclass
       AND contype = 'c'
       AND pg_get_constraintdef(oid) LIKE '%account_lock%'
       AND pg_get_constraintdef(oid) NOT LIKE '%failure_class%';
    EXECUTE format('ALTER TABLE diagnostic_events DROP CONSTRAINT %I', kind_constraint);
END
$$;

ALTER TABLE diagnostic_events
    ADD CONSTRAINT diagnostic_events_kind_allowed
    CHECK (kind IN ('ai', 'login', 'account_lock', 'book_source'));

ALTER TABLE diagnostic_events DROP CONSTRAINT diagnostic_events_failure_class_allowed;

ALTER TABLE diagnostic_events ADD CONSTRAINT diagnostic_events_failure_class_allowed CHECK (
    failure_class IS NULL OR
    (kind = 'ai' AND failure_class IN ('connection_failed', 'timeout', 'invalid_key',
        'quota_exceeded', 'rate_limited', 'service_error', 'invalid_output',
        'classroom_ai_unavailable')) OR
    (kind = 'book_source' AND failure_class IN ('connection_failed', 'timeout', 'invalid_key',
        'quota_exceeded', 'rate_limited', 'service_error', 'invalid_output')) OR
    (kind = 'login' AND failure_class IN ('invalid_credentials', 'account_disabled', 'account_locked')) OR
    (kind = 'account_lock' AND failure_class = 'account_locked')
);
