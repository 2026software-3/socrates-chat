-- 系統 AI 設定（管理者在介面管理系統 key 與模型）。系統用途 = 沒有教室的個人練習、過渡中的教室、翻譯、
-- 主張分組、維度與公告翻譯等不屬於任何教師的 AI 呼叫。
-- 沒有設定（或沒有可用的 key）時照舊使用主機上的 .apikeylist，所以這張表是空的時行為完全不變。

CREATE TABLE system_api_keys (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider          text NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    ciphertext        bytea NOT NULL CHECK (octet_length(ciphertext) > 0),
    encryption_key_id text NOT NULL CHECK (encryption_key_id <> ''),
    mask              text NOT NULL CHECK (char_length(mask) <= 16),
    -- key 的 SHA-256；全域唯一，用來拒絕重複，不能反推 key。
    fingerprint       bytea NOT NULL UNIQUE CHECK (octet_length(fingerprint) = 32),
    status            text NOT NULL DEFAULT 'ok'
        CHECK (status IN ('ok', 'invalid', 'rate_limited', 'quota_exhausted')),
    status_at         timestamptz,
    retry_after_secs  integer CHECK (retry_after_secs > 0),
    CHECK ((status = 'ok') = (status_at IS NULL)),
    created_by        uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX system_api_keys_provider_idx ON system_api_keys (provider, created_at);

-- 只有一列：系統用途使用的供應商與模型。
CREATE TABLE system_ai_settings (
    singleton  boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    provider   text NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    model      text NOT NULL CHECK (btrim(model) <> '' AND char_length(model) <= 200),
    updated_by uuid REFERENCES users (id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- 系統用途的 AI 用量：只存次數與 token，不存金額、內容或 key。「日」以 Asia/Taipei 計。
CREATE TABLE system_ai_usage (
    key_id        uuid   NOT NULL REFERENCES system_api_keys (id) ON DELETE CASCADE,
    provider      text   NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    model         text   NOT NULL CHECK (btrim(model) <> '' AND char_length(model) <= 200),
    day           date   NOT NULL,
    requests      bigint NOT NULL DEFAULT 0 CHECK (requests >= 0),
    input_tokens  bigint NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens bigint NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    unreported_requests bigint NOT NULL DEFAULT 0 CHECK (unreported_requests >= 0),
    PRIMARY KEY (key_id, provider, model, day)
);

-- 稽核：沿用現有約束的內容再加上新的值（只往前）。對象欄位記供應商識別碼，不記 key 或遮蔽識別。
DO $$
DECLARE audit_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT audit_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format(
        'ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action IN (%L, %L, %L, %L, %L))',
        audit_rule, 'system_key_added', 'system_key_replaced', 'system_key_deleted',
        'system_ai_model_set', 'system_ai_model_cleared'
    );
END;
$$;
