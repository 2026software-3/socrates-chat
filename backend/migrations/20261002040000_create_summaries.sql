-- F-08.3 / F-08.4：對話結束後的 AI 總結（S-03.3、S-02.2）
-- 總結屬敏感欄位：教師看得到（不含原文），管理者介面一律遮蔽；學生不能修改。

-- 主張分組（F-15，暫定）：同一題目下，AI 把意思相近的主張歸成一群並命名。
-- 分組結果存起來，不在每次看儀表板時重算（同一份資料才不會今天和明天分得不一樣）。
-- 以對話複製來的題目標題分組（來源之後異動不影響）。
CREATE TABLE claim_groups (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    topic_title text NOT NULL,
    name        text NOT NULL CHECK (btrim(name) <> ''),
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (topic_title, name)
);

CREATE TABLE summaries (
    conversation_id uuid PRIMARY KEY REFERENCES conversations (id) ON DELETE CASCADE,
    -- pending：產生中；ready：完成；failed：AI 失敗或結果不完整，可重試
    status          text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'failed')),
    -- 第幾次產生；重試時加 1。背景任務只能寫入自己那一代，避免舊任務覆蓋新結果
    attempt         integer NOT NULL DEFAULT 1,
    -- 主要立場與核心理由（F-08.3）；思考轉折先不記錄
    stance          text,
    reasons         text,
    -- 學生最終主張的一句短句（約 30 字內，用對話語言）；論點分布（F-15，暫定）的依據，模型沒給時為空
    claim           text,
    -- 這句主張所屬的分組；分組失敗或尚未分組時為空，分組被刪除時也回到空
    claim_group_id  uuid REFERENCES claim_groups (id) ON DELETE SET NULL,
    -- 學生論證主要依循的倫理學派（S-04.2 的 6 個維度，加上 other）；只是輔助標籤
    framework       text CHECK (framework IN
                        ('utilitarianism', 'deontology', 'virtue', 'contractarianism', 'care',
                         'existentialism', 'other')),
    -- AI 為這份論點總結在 6 個學派維度上的分數（S-04.2，暫定）：JSON 物件，鍵為 6 個學派，
    -- 值為 0–5 的整數（0 完全沒用到、5 是論點核心）；模型沒給完整 6 項時為空
    framework_scores jsonb CHECK (jsonb_typeof(framework_scores) = 'object'),
    rules_version   text,
    started_at      timestamptz NOT NULL DEFAULT now(),
    completed_at    timestamptz,
    CHECK (status <> 'ready' OR (stance IS NOT NULL AND reasons IS NOT NULL))
);

CREATE INDEX summaries_ready_idx ON summaries (completed_at DESC) WHERE status = 'ready';
