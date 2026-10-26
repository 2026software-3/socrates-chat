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
    -- 活動必須選題庫題目（S-04.7）；活動已有對話後不能更換題目，由 API 檢查
    topic_id    uuid NOT NULL REFERENCES topics (id),
    -- 活動只屬於這間教室；教室不能刪除，所以沒有連動刪除
    classroom_id uuid NOT NULL REFERENCES classrooms (id),
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
