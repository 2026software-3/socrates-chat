-- F-01.1-DB：Google 登入與 session（S-01.1、S-01.2、S-08.2）

CREATE TABLE users (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Google 帳號 ID（OIDC `sub`）；一個 Google 帳號對應一個使用者
    google_sub   text NOT NULL UNIQUE CHECK (google_sub <> ''),
    -- 每次登入以 Google 回傳值更新；一律小寫
    email        text NOT NULL CHECK (email = lower(email) AND email <> ''),
    display_name text,
    -- 首次登入時依 ADMIN_EMAILS 授予，之後以資料庫為準
    is_admin     boolean NOT NULL DEFAULT false,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- 電子郵件用於比對修課名單與教師清單
CREATE INDEX users_email_idx ON users (email);

CREATE TABLE sessions (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 256 位元隨機 token 的 SHA-256 雜湊；不存原值
    token_hash   bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);

-- OAuth 授權流程暫存：state、nonce、PKCE verifier（10 分鐘有效，單次使用）
CREATE TABLE oauth_login_states (
    state         text PRIMARY KEY,
    nonce         text NOT NULL,
    pkce_verifier text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
