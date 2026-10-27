-- F-01.1-DB、F-01.5-DB：Google 與內建電子郵件＋密碼登入、session（S-01.1、S-01.2、S-08.2）

CREATE TABLE users (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Google 帳號 ID（OIDC `sub`）；一個 Google 帳號對應一個使用者。Google 登入時才綁定，因此可以為空
    google_sub   text UNIQUE CHECK (google_sub <> ''),
    -- 一個電子郵件對應一個使用者；Google 登入時以回傳值更新；一律小寫
    email        text UNIQUE CHECK (email = lower(email) AND email <> ''),
    -- 學生以學號識別；Google 登入的未開通帳號及教職員可沒有學號。
    student_id   text UNIQUE CHECK (student_id ~ '^[0-9]{8}[A-Z]$'),
    UNIQUE (id, student_id),
    imported_name text CHECK (imported_name <> '' AND char_length(imported_name) <= 30),
    CHECK (email IS NOT NULL OR student_id IS NOT NULL),
    -- Argon2id 的 PHC 字串；空表示尚未設定密碼（只能用 Google 登入）
    password_hash text CHECK (password_hash <> ''),
    -- 臨時密碼登入後必須先更改
    must_change_password boolean NOT NULL DEFAULT false,
    display_name text,
    -- 介面與新對話使用的語言。帳號可能在本人首次登入前由管理者建立；
    -- language_initialized 用來區分「尚待 Accept-Language 初始化」與使用者已確認的 zh-TW。
    language     text NOT NULL DEFAULT 'zh-TW' CHECK (language IN ('zh-TW', 'en', 'es')),
    language_initialized boolean NOT NULL DEFAULT false,
    -- 首次登入時依 ADMIN_EMAILS 授予，之後以資料庫為準
    is_admin     boolean NOT NULL DEFAULT false,
    CHECK (student_id IS NULL OR (imported_name IS NOT NULL AND NOT is_admin)),
    -- 帳號停用（F-01.4，S-01.3）：只有管理者可操作；停用時清除所有 session，資料全部保留，復原後照舊
    disabled_at  timestamptz,
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

-- 內建登入的失敗限流：以（電子郵件, 來源 IP）為單位計（#295）。電子郵件不論帳號是否存在都計，避免洩漏；
-- 別的 IP 亂試不會鎖住本人，同一個 IP（例如校園 NAT）下不同信箱也互不影響。
-- ip 是 client_ip::rate_limit_key 的結果（IPv6 為 /64 前綴），沒有連線資訊時為空字串；
-- ip = '*' 的列是這個信箱在所有 IP 的累計，用來限制分散式的猜密碼。
-- IP 屬個資，保存期限待與 S-02.5 對齊；目前每次記錄失敗時順便清掉一天前的舊資料。
CREATE TABLE login_failures (
    email          text NOT NULL CHECK (email = lower(email)),
    ip             text NOT NULL DEFAULT '',
    failures       integer NOT NULL DEFAULT 0,
    locked_until   timestamptz,
    last_failed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (email, ip)
);
