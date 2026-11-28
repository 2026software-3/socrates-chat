-- 稽核：新增管理者（admin_added）。沿用現有約束的內容再加上新的值（只往前）。
DO $$
DECLARE audit_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT audit_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format(
        'ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action IN (%L))',
        audit_rule, 'admin_added'
    );
END;
$$;
