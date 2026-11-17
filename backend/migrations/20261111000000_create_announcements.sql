-- F-34 / S-15：公告與維護模式。

-- 公告：管理者只寫 zh-TW，en／es 由系統翻譯；翻譯失敗時對應語言留空並列入 pending_languages。
CREATE TABLE announcements (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind              text NOT NULL CHECK (kind IN ('general', 'downtime')),
    text_zh_tw        text NOT NULL CHECK (btrim(text_zh_tw) <> ''),
    text_en           text,
    text_es           text,
    pending_languages text[] NOT NULL DEFAULT '{}',
    starts_at         timestamptz NOT NULL DEFAULT now(),
    -- 不填表示到管理者停用或刪除為止
    ends_at           timestamptz,
    enabled           boolean NOT NULL DEFAULT true,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    CHECK (ends_at IS NULL OR ends_at > starts_at)
);

CREATE INDEX announcements_created_idx ON announcements (created_at DESC);

-- 維護模式旗標：只有一列，存在資料庫讓後端重啟後仍然有效。
CREATE TABLE maintenance_mode (
    singleton  boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    enabled    boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid REFERENCES users (id) ON DELETE SET NULL
);

INSERT INTO maintenance_mode DEFAULT VALUES;

-- 保留其他切片先前新增的稽核動作，再加上本功能的動作。
DO $$
DECLARE existing_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT existing_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format(
        'ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action IN (%L, %L, %L, %L, %L))',
        existing_rule,
        'announcement_created', 'announcement_updated', 'announcement_deleted',
        'maintenance_enabled', 'maintenance_disabled'
    );
END;
$$;
