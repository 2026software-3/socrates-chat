-- 內建電子郵件＋密碼登入（S-01.1、S-01.2、S-08.2）
-- 一個電子郵件對應一個使用者；Google 登入時才綁定 google_sub，因此可以為空。

ALTER TABLE users ALTER COLUMN google_sub DROP NOT NULL;

-- Argon2id 的 PHC 字串；空表示尚未設定密碼（只能用 Google 登入）
ALTER TABLE users ADD COLUMN password_hash text CHECK (password_hash <> '');
-- 臨時密碼登入後必須先更改
ALTER TABLE users ADD COLUMN must_change_password boolean NOT NULL DEFAULT false;

CREATE UNIQUE INDEX users_email_unique ON users (email);
DROP INDEX users_email_idx;

-- 內建登入的失敗限流：依電子郵件計（不論帳號是否存在，避免洩漏帳號是否存在）
CREATE TABLE login_failures (
    email          text PRIMARY KEY CHECK (email = lower(email)),
    failures       integer NOT NULL DEFAULT 0,
    locked_until   timestamptz,
    last_failed_at timestamptz NOT NULL DEFAULT now()
);

-- 用臨時密碼登入的 session：更改密碼前只能使用 /api/me 與改密碼（S-08.2）。
-- 旗標放在 session 而不是使用者：同一個人改用 Google 登入並不知道臨時密碼，不需要被擋。
ALTER TABLE sessions ADD COLUMN must_change_password boolean NOT NULL DEFAULT false;
