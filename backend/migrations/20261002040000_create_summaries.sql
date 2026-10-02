-- F-08.3 / F-08.4：對話結束後的 AI 總結（S-03.3、S-02.2）
-- 總結屬敏感欄位：教師看得到（不含原文），管理者介面一律遮蔽；學生不能修改。

CREATE TABLE summaries (
    conversation_id uuid PRIMARY KEY REFERENCES conversations (id) ON DELETE CASCADE,
    -- pending：產生中；ready：完成；failed：AI 失敗或結果不完整，可重試
    status          text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'failed')),
    -- 第幾次產生；重試時加 1。背景任務只能寫入自己那一代，避免舊任務覆蓋新結果
    attempt         integer NOT NULL DEFAULT 1,
    -- 主要立場、核心理由、重要轉折與修正（F-08.3）
    stance          text,
    reasons         text,
    turning_points  text,
    rules_version   text,
    started_at      timestamptz NOT NULL DEFAULT now(),
    completed_at    timestamptz,
    CHECK (status <> 'ready' OR (stance IS NOT NULL AND reasons IS NOT NULL AND turning_points IS NOT NULL))
);

CREATE INDEX summaries_ready_idx ON summaries (completed_at DESC) WHERE status = 'ready';

-- 開始對話當下是否以「修課學生」身分（教師或管理者自己試用的對話不列入教師的學生總結清單）。
-- 事後被移出名單不影響，教師仍看得到過去的總結（S-01.4）。
ALTER TABLE conversations ADD COLUMN as_student boolean NOT NULL DEFAULT true;
