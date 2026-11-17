-- F-36 / S-17：客訴。描述是使用者自己輸入、要給管理者閱讀的文字（S-02.2 的例外），
-- 但不得進日誌、稽核日誌或送往 AI；信件另外不含學號、電子郵件與對話原文。

CREATE TABLE complaints (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- 帳號刪除時一併刪除該帳號送出的客訴（S-17.1、S-02.5）
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    category        text NOT NULL CHECK (category IN ('connection', 'voice', 'content', 'other')),
    -- 去掉前後空白後非空，上限 2000 字（字元數）
    description     text NOT NULL CHECK (btrim(description) <> '' AND char_length(description) <= 2000),
    -- 只在從對話頁開啟表單時帶入；對話被刪除後只剩空值，不帶任何原文
    conversation_id uuid REFERENCES conversations (id) ON DELETE SET NULL,
    -- 送出當下的身分（學生或教師），給信件與管理介面顯示
    submitter_role  text NOT NULL CHECK (submitter_role IN ('student', 'teacher')),
    status          text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved')),
    mail_status     text NOT NULL DEFAULT 'pending' CHECK (mail_status IN ('pending', 'sent', 'failed', 'skipped')),
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- 每人每小時、每日的送出次數（限流）
CREATE INDEX complaints_user_created_idx ON complaints (user_id, created_at DESC);
CREATE INDEX complaints_created_idx ON complaints (created_at DESC);

-- 保留其他切片先前新增的稽核動作，再加上本功能的動作。
DO $$
DECLARE existing_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT existing_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format(
        'ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action IN (%L, %L))',
        existing_rule,
        'complaint_resolved', 'complaint_reopened'
    );
END;
$$;
