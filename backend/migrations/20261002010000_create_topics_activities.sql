-- F-02 / F-03 / F-04：題目庫與討論活動
-- 系統只服務一門課程，所以沒有班級或學期欄位。

CREATE TABLE topics (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    title       text NOT NULL CHECK (btrim(title) <> ''),
    description text NOT NULL DEFAULT '',
    category    text,
    -- 停用後學生不再看得到，既有對話不受影響
    is_active   boolean NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE activities (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    title       text NOT NULL CHECK (btrim(title) <> ''),
    -- 教師給學生的說明／討論題目內容
    description text NOT NULL DEFAULT '',
    -- 活動必須選題庫題目（S-04.7）；活動已有對話後不能更換題目，由 API 檢查
    topic_id    uuid NOT NULL REFERENCES topics (id),
    -- draft：草稿；published：開放學生選擇；closed：停止開放
    status      text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'closed')),
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX activities_status_idx ON activities (status);
