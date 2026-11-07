-- F-29.1、S-04.7：活動與題目改為多對多（含順序），移除 activities.topic_id。
-- NTNU 已是正式環境，且已套用過 20261002010000 的舊版（活動只有一個 topic_id），
-- 所以不能改那個檔案；已套用的 migration 不修改，由這個檔案把既有資料搬過來。

CREATE TABLE activity_topics (
    activity_id uuid NOT NULL REFERENCES activities (id) ON DELETE CASCADE,
    topic_id    uuid NOT NULL REFERENCES topics (id),
    position    integer NOT NULL CHECK (position >= 0 AND position < 5),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (activity_id, topic_id),
    CONSTRAINT activity_topics_position_unique UNIQUE (activity_id, position)
);

CREATE INDEX activity_topics_topic_idx ON activity_topics (topic_id);

-- 既有活動原本的題目成為第一個題目
INSERT INTO activity_topics (activity_id, topic_id, position)
SELECT id, topic_id, 0 FROM activities;

ALTER TABLE activities DROP COLUMN topic_id;
