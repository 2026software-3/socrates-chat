-- F-06.1：對話與訊息（S-02.5、S-03.2、S-03.3、S-05.4、S-12.1）
-- 訊息內容屬敏感欄位（S-02.2）：只有擁有者讀得到，教師與管理者的輸出一律遮蔽。

CREATE TABLE conversations (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 來源：教師活動或題目庫題目；來源之後被刪除或停用不影響既有對話
    activity_id  uuid REFERENCES activities (id) ON DELETE SET NULL,
    topic_id     uuid REFERENCES topics (id) ON DELETE SET NULL,
    -- 開始時複製的題目內容，之後來源異動不會改變這場對話
    title        text NOT NULL,
    description  text NOT NULL DEFAULT '',
    -- 開始時複製使用者的語言設定，進行中不變（S-12.1）
    language     text NOT NULL DEFAULT 'zh-TW' CHECK (language IN ('zh-TW', 'en', 'es')),
    status       text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'ended')),
    -- 追問階段 1 釐清 / 2 論證 / 3 挑戰，只進不退，存在後端（S-03.2）
    stage        smallint NOT NULL DEFAULT 1 CHECK (stage BETWEEN 1 AND 3),
    -- 學生每送出一則訊息算 1 回合（S-03.3）
    turn_count   integer NOT NULL DEFAULT 0,
    created_at   timestamptz NOT NULL DEFAULT now(),
    ended_at     timestamptz
);

CREATE INDEX conversations_user_idx ON conversations (user_id, created_at DESC);

CREATE TABLE messages (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id uuid NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    seq             bigint GENERATED ALWAYS AS IDENTITY,
    role            text NOT NULL CHECK (role IN ('student', 'ai')),
    content         text NOT NULL,
    -- 學生訊息的來源：文字、瀏覽器語音、OpenAI 語音（S-05.4）
    source          text NOT NULL DEFAULT 'text' CHECK (source IN ('text', 'speech-browser', 'speech-openai')),
    -- AI 回覆的規則版本，例如 v1/zh-TW（S-03.2）
    rules_version   text,
    -- AI 追問類型（S-03.2）
    question_type   text,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX messages_conversation_idx ON messages (conversation_id, seq);
