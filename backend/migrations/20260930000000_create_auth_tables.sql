-- F-01.1-DB、F-01.5-DB：Google 與內建電子郵件＋密碼登入、session（S-01.1、S-01.2、S-08.2）

CREATE TABLE users (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Google 帳號 ID（OIDC `sub`）；一個 Google 帳號對應一個使用者。Google 登入時才綁定，因此可以為空
    google_sub   text UNIQUE CHECK (google_sub <> ''),
    -- 一個電子郵件對應一個使用者；Google 登入時以回傳值更新；一律小寫
    email        text NOT NULL UNIQUE CHECK (email = lower(email) AND email <> ''),
    -- Argon2id 的 PHC 字串；空表示尚未設定密碼（只能用 Google 登入）
    password_hash text CHECK (password_hash <> ''),
    -- 臨時密碼登入後必須先更改
    must_change_password boolean NOT NULL DEFAULT false,
    display_name text,
    -- 首次登入時依 ADMIN_EMAILS 授予，之後以資料庫為準
    is_admin     boolean NOT NULL DEFAULT false,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 256 位元隨機 token 的 SHA-256 雜湊；不存原值
    token_hash   bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    -- 用臨時密碼登入的 session：更改密碼前只能使用 /api/me 與改密碼（S-08.2）。
    -- 旗標放在 session 而不是使用者：同一個人改用 Google 登入並不知道臨時密碼，不需要被擋。
    must_change_password boolean NOT NULL DEFAULT false
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);

-- OAuth 授權流程暫存：state、nonce、PKCE verifier（10 分鐘有效，單次使用）
CREATE TABLE oauth_login_states (
    state         text PRIMARY KEY,
    nonce         text NOT NULL,
    pkce_verifier text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);

-- 內建登入的失敗限流：依電子郵件計（不論帳號是否存在，避免洩漏帳號是否存在）
CREATE TABLE login_failures (
    email          text PRIMARY KEY CHECK (email = lower(email)),
    failures       integer NOT NULL DEFAULT 0,
    locked_until   timestamptz,
    last_failed_at timestamptz NOT NULL DEFAULT now()
);
