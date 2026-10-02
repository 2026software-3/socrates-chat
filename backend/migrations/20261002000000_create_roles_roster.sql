-- F-01.2 / F-25：教師清單與修課名單（S-01.3、S-01.4）
-- 兩者都以電子郵件為鍵（一律小寫），因此對方尚未登入過也能先加入；
-- 角色每次請求由 users.email 對照這兩張表決定，移除後立即失效。

CREATE TABLE teachers (
    email      text PRIMARY KEY CHECK (email = lower(email) AND email <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE enrollments (
    email      text PRIMARY KEY CHECK (email = lower(email) AND email <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);
