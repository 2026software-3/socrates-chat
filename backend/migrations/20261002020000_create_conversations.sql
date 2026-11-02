-- F-06.1、F-07、F-08：對話與訊息（S-02.5、S-03.2、S-03.3、S-05.4、S-12.1）
-- 訊息內容屬敏感欄位（S-02.2）：只有擁有者讀得到，教師與管理者的輸出一律遮蔽。

CREATE TABLE conversations (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 來源：教師活動或題目庫題目；刪除活動時一併刪除其對話（S-14），題目停用不影響既有對話
    activity_id  uuid REFERENCES activities (id) ON DELETE CASCADE,
    topic_id     uuid REFERENCES topics (id) ON DELETE SET NULL,
    -- 教室歸屬（S-14）：活動對話在開始時取自活動所在的教室，建立後不再更改，
    -- 從題目庫直接開始的個人練習為空，教師看不到、不納入任何教室的統計。
    -- 刪除教室時一併刪除其中的對話（S-14）。
    classroom_id uuid REFERENCES classrooms (id) ON DELETE CASCADE,
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
    -- 第 3 階段中學生已回應反例或對立觀點時由後端設為 true；之後 AI 提議收尾（S-03.3）
    converge_ready boolean NOT NULL DEFAULT false,
    -- 正在產生 AI 回覆的時間；同一場對話同時間只允許一條回覆串流。
    -- 超過逾時上限仍未清除（例如服務重啟）視為過期，可重新產生。
    generating_since timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    ended_at     timestamptz,
    -- 學生封存：只從自己的清單隱藏，不影響教師端總結、彙整與點數；可復原（S-14）
    archived_at  timestamptz
);

CREATE INDEX conversations_user_idx ON conversations (user_id, created_at DESC, id);
-- 總結列表（GET /api/summaries）依 ended_at DESC, id 做 keyset 分頁，只列已結束的對話。
CREATE INDEX conversations_ended_idx ON conversations (ended_at DESC, id) WHERE ended_at IS NOT NULL;
CREATE INDEX conversations_classroom_idx ON conversations (classroom_id) WHERE classroom_id IS NOT NULL;
-- 刪除活動時的連動刪除與對話計數（S-14）
CREATE INDEX conversations_activity_idx ON conversations (activity_id) WHERE activity_id IS NOT NULL;

-- 每位學生在每個活動至多一場對話，進行中或已結束都算（S-04.7）；刪除該場對話後可重新開始。
-- 從題目庫直接開始的個人練習沒有活動，不受限制。
CREATE UNIQUE INDEX conversations_user_activity_uniq
    ON conversations (user_id, activity_id) WHERE activity_id IS NOT NULL;

CREATE TABLE messages (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id uuid NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    seq             bigint GENERATED ALWAYS AS IDENTITY,
    role            text NOT NULL CHECK (role IN ('student', 'ai')),
    content         text NOT NULL,
    -- 學生訊息的來源：文字、瀏覽器語音、OpenAI 語音（S-05.4）、語音對話實驗（cascade）
    source          text NOT NULL DEFAULT 'text' CHECK (source IN ('text', 'speech-browser', 'speech-openai', 'speech-cascade')),
    -- AI 回覆的規則版本，例如 v1/zh-TW（S-03.2）
    rules_version   text,
    -- AI 追問類型（S-03.2）
    question_type   text,
    -- 模型對「是否達到進入下一階段條件」的判斷理由，供稽核與評估，不回傳給學生（S-03.2）
    advance_reason  text,
    -- 語音回覆被打斷時學生聽到的字元數；content 留完整回覆供稽核，0 表示一個字都沒聽到
    heard_chars     integer,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX messages_conversation_idx ON messages (conversation_id, seq);
