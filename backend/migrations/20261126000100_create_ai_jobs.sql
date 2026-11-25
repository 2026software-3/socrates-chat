-- S-20：背景執行的 AI 工作（教師重新分組等）。各功能自己的狀態表（總結、雷達、論證樹）不動，
-- 這張表給新背景化的功能共用。
-- 不存訊息、AI 總結或供應商診斷：scope 只放識別碼，result 只放結果摘要，error_code 是固定代碼。

CREATE TABLE ai_jobs (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind          text NOT NULL,
    -- 發起者；只有發起者能查詢
    owner_user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 同一範圍互斥的鍵，例如 regroup:{user}:{activity}:{topic}；未結束的工作同鍵只能有一筆
    scope_key     text NOT NULL,
    -- 來源識別碼（activity_id、topic_id、classroom_id、conversation_id），給前端連回來源頁
    scope         jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(scope) = 'object'),
    status        text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'ready', 'failed')),
    result        jsonb,
    error_code    text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz,
    CHECK ((status IN ('ready', 'failed')) = (finished_at IS NOT NULL))
);

CREATE UNIQUE INDEX ai_jobs_active_scope_idx ON ai_jobs (scope_key) WHERE status IN ('pending', 'running');
CREATE INDEX ai_jobs_owner_idx ON ai_jobs (owner_user_id, created_at DESC);
-- 清除過期的已結束工作
CREATE INDEX ai_jobs_finished_idx ON ai_jobs (finished_at) WHERE finished_at IS NOT NULL;
