-- F-40.1-DB (#591): 教師自備的 AI 供應商 API key（S-03.1，#453）。
-- 只往前：新增資料表，不動任何既有資料。
--
-- 只存密文、遮蔽識別與指紋，不存明文。加密演算法與金鑰輪替方式仍待需求方確認（見 teacher_key_crypto.rs）。
-- 供應商是文字、由應用層的名單驗證（F-40.3），不用資料庫 enum，之後新增供應商不必改 schema。
CREATE TABLE teacher_api_keys (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- 帳號被刪除時連 key 一併刪除；帳號只是停用時 key 保留（由路由層決定是否使用）。
    owner_id          uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider          text NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    -- nonce＋密文＋驗證標籤；`encryption_key_id` 指出用哪一把加密金鑰，供輪替時並存。
    ciphertext        bytea NOT NULL CHECK (octet_length(ciphertext) > 0),
    encryption_key_id text NOT NULL CHECK (encryption_key_id <> ''),
    -- 畫面上顯示的遮蔽識別，例如「…abcd」。
    mask              text NOT NULL CHECK (char_length(mask) <= 16),
    -- key 的 SHA-256；全域唯一，用來拒絕重複（含不同教師），不能反推 key。
    fingerprint       bytea NOT NULL UNIQUE CHECK (octet_length(fingerprint) = 32),
    -- 最近一次實際使用或測試得到的狀態，供「key 無法使用或受到限制」的顯示。
    status            text NOT NULL DEFAULT 'ok'
        CHECK (status IN ('ok', 'invalid', 'rate_limited', 'quota_exhausted')),
    status_at         timestamptz,
    CHECK ((status = 'ok') = (status_at IS NULL)),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX teacher_api_keys_owner_provider_idx ON teacher_api_keys (owner_id, provider, created_at);
