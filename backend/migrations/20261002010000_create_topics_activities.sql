-- F-02 / F-03 / F-04：題目庫與討論活動
-- 系統只服務一門課程，沒有學期欄位；題目庫是全域的，活動屬於教室（S-14，F-28.3）。

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
    -- 活動只屬於這間教室；刪除教室時一併刪除活動（S-14）
    classroom_id uuid NOT NULL REFERENCES classrooms (id) ON DELETE CASCADE,
    -- 封存：學生看不到也不能開新對話，既有對話與總結照舊；可復原（S-14）
    is_archived boolean NOT NULL DEFAULT false,
    -- draft：草稿；published：開放學生選擇；closed：停止開放
    status      text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'closed')),
    opens_at    timestamptz,
    closes_at   timestamptz,
    CONSTRAINT activities_schedule_check CHECK (closes_at > opens_at),
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX activities_status_idx ON activities (status);
CREATE INDEX activities_classroom_idx ON activities (classroom_id);
-- 活動列表（GET /api/activities）依 created_at DESC, id 做 keyset 分頁。
CREATE INDEX activities_created_idx ON activities (created_at DESC, id);

-- 活動與題目的多對多關聯（含順序，F-29.1、S-04.7）
CREATE TABLE activity_topics (
    activity_id uuid NOT NULL REFERENCES activities (id) ON DELETE CASCADE,
    topic_id    uuid NOT NULL REFERENCES topics (id),
    position    integer NOT NULL CHECK (position >= 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (activity_id, topic_id),
    CONSTRAINT activity_topics_position_unique UNIQUE (activity_id, position)
);

CREATE INDEX activity_topics_topic_idx ON activity_topics (topic_id);
