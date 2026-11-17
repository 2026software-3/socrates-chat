-- 自己更改密碼的稽核動作（S-18）。保留先前所有切片新增的動作，再加上 change_password。
DO $$
DECLARE existing_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT existing_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format(
        'ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action = %L)',
        existing_rule,
        'change_password'
    );
END;
$$;
