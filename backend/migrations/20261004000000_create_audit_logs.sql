-- F-22.1：稽核日誌（S-01.3、S-02.5）。目前記錄帳號停用、復原與學生帳號刪除。
-- 只記錄操作者、動作、對象與選填原因；不含任何對話或總結內容。

CREATE TABLE audit_logs (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- 操作者；操作者帳號之後被刪除時保留紀錄
    actor_id     uuid REFERENCES users (id) ON DELETE SET NULL,
    action       text NOT NULL CHECK (action IN ('account_disabled', 'account_enabled', 'student_deleted')),
    -- 對象的電子郵件：帳號被刪除後仍需留下是誰
    target_email text NOT NULL,
    reason       text,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX audit_logs_created_idx ON audit_logs (created_at DESC);
