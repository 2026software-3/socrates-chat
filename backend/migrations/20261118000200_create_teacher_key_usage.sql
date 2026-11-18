-- F-40.8-DB (#602): 教師 key 的 AI 用量，依（key、教室、供應商、模型、日）累計（S-03.1，#453）。
-- 只往前：新增資料表。不計金額，所以沒有金額欄位。
-- 「日」以 Asia/Taipei 的日曆日計（與 tasks／achievements 一致；待需求方確認）。
--
-- 刪除 key 連帶刪除其用量（教室總計由各筆加總，隨之扣除）；更換 key 的密文是 UPDATE 同一列 key，歷史保留；
-- 刪除教室也連帶刪除其用量。
CREATE TABLE teacher_key_usage (
    key_id        uuid   NOT NULL REFERENCES teacher_api_keys (id) ON DELETE CASCADE,
    classroom_id  uuid   NOT NULL REFERENCES classrooms (id) ON DELETE CASCADE,
    provider      text   NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    model         text   NOT NULL CHECK (btrim(model) <> '' AND char_length(model) <= 200),
    day           date   NOT NULL,
    requests      bigint NOT NULL DEFAULT 0 CHECK (requests >= 0),
    input_tokens  bigint NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens bigint NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    -- 供應商沒有回報 token 的請求數；這些請求不估算 token。
    unreported_requests bigint NOT NULL DEFAULT 0 CHECK (unreported_requests >= 0),
    PRIMARY KEY (key_id, classroom_id, provider, model, day)
);

CREATE INDEX teacher_key_usage_classroom_idx ON teacher_key_usage (classroom_id, day);
