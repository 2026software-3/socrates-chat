-- F-28.1：教師建立的教室；名稱全域唯一（忽略前後空白與大小寫，S-14）。
CREATE TABLE classrooms (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL CHECK (
        regexp_replace(name, '^[[:space:]]+|[[:space:]]+$', '', 'g') <> ''
    ),
    year_label  text,
    description text,
    created_by  uuid NOT NULL REFERENCES users (id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX classrooms_normalized_name_unique
    ON classrooms (lower(regexp_replace(name, '^[[:space:]]+|[[:space:]]+$', '', 'g')));

CREATE INDEX classrooms_created_by_idx ON classrooms (created_by);
