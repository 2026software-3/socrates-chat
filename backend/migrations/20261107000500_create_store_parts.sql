-- S-07.4：商品清單與素材尚待 S-07.1 定案；此處不預設正式部件。
CREATE TABLE store_parts (
    id text PRIMARY KEY CHECK (id <> ''),
    name_zh_tw text NOT NULL CHECK (btrim(name_zh_tw) <> ''),
    name_en text NOT NULL CHECK (btrim(name_en) <> ''),
    name_es text NOT NULL CHECK (btrim(name_es) <> ''),
    price integer NOT NULL CHECK (price >= 0),
    enabled boolean NOT NULL DEFAULT true
);

-- 保留其他切片先前新增的稽核動作。
DO $$
DECLARE existing_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT existing_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format('ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action = %L)', existing_rule, 'part_updated');
END;
$$;
