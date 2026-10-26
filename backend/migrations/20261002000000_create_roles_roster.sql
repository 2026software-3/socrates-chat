-- F-01.2：教師清單（S-01.3）
-- 以電子郵件為鍵（一律小寫），因此對方尚未登入過也能先加入；
-- 角色每次請求由 users.email 對照這張表決定，移除後立即失效。
-- 修課名單（enrollments）屬於教室，建立在 20261001000000_create_classrooms.sql。

CREATE TABLE teachers (
    email      text PRIMARY KEY CHECK (email = lower(email) AND email <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);
