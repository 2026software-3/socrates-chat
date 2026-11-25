-- 語音節點登錄（S-19）：管理者新增、暫停、移除 cascade 節點，不必重啟後端。
-- 健康狀態與場次數不存資料庫（探測結果與記憶體計數）。
CREATE TABLE voice_nodes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE,
    realtime_url text NOT NULL CHECK (realtime_url ~ '^wss?://'),
    -- 節點密鑰的 SHA-256；明文只在建立與輪換時回傳一次
    token_hash bytea NOT NULL UNIQUE,
    max_sessions integer NOT NULL CHECK (max_sessions >= 1),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'draining', 'disabled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- 稽核動作：保留先前所有切片新增的動作，再加上語音節點的四個。
DO $$
DECLARE existing_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT existing_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format(
        'ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action IN (%L, %L, %L, %L))',
        existing_rule,
        'voice_node_create',
        'voice_node_update',
        'voice_node_rotate_token',
        'voice_node_delete'
    );
END;
$$;
