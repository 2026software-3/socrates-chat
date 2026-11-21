-- F-40.9 (#605): 管理者刪除教師 key 後通知教師並寫稽核紀錄。
-- 只往前：沿用現有約束的內容再加上新的值，不移除其他功能加入的值。
DO $$
DECLARE audit_rule text;
DECLARE kind_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT audit_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format('ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action = %L)', audit_rule, 'teacher_key_deleted');

    SELECT pg_get_expr(conbin, conrelid) INTO STRICT kind_rule
    FROM pg_constraint WHERE conrelid = 'student_notifications'::regclass AND conname = 'student_notifications_kind_check';
    ALTER TABLE student_notifications DROP CONSTRAINT student_notifications_kind_check;
    EXECUTE format('ALTER TABLE student_notifications ADD CONSTRAINT student_notifications_kind_check CHECK ((%s) OR kind = %L)', kind_rule, 'teacher_api_key_deleted');
END;
$$;
