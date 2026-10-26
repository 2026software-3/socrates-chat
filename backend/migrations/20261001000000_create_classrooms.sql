-- F-28.1：教師建立的教室；名稱全域唯一（忽略前後空白與大小寫，S-14）。
-- 活動（activities）與對話（conversations）都參照教室，所以這個檔案排在它們之前。
CREATE TABLE classrooms (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL CHECK (
        regexp_replace(name, '^[[:space:]]+|[[:space:]]+$', '', 'g') <> ''
        AND char_length(name) <= 255
    ),
    year_label  text CHECK (char_length(year_label) <= 100),
    description text CHECK (char_length(description) <= 2000),
    is_archived boolean NOT NULL DEFAULT false,
    created_by  uuid NOT NULL REFERENCES users (id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX classrooms_normalized_name_unique
    ON classrooms (lower(regexp_replace(name, '^[[:space:]]+|[[:space:]]+$', '', 'g')));

CREATE INDEX classrooms_created_by_idx ON classrooms (created_by);

-- F-28.2：修課名單以教室為單位（S-14、S-01.4）。
-- 同一電子郵件可同時在多個教室的名單，帳號仍只有一個（users.email 唯一）；
-- 以電子郵件為鍵（一律小寫），對方尚未登入過也能先加入。學生角色每次請求由
-- users.email 對照任一教室的名單決定，移出所有名單後立即失效。
CREATE TABLE enrollments (
    classroom_id uuid NOT NULL REFERENCES classrooms (id),
    email        text NOT NULL CHECK (email = lower(email) AND email <> ''),
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (classroom_id, email)
);

-- 「這個帳號是否在任一教室的名單內」每次請求都會查
CREATE INDEX enrollments_email_idx ON enrollments (email);
