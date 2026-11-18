-- F-40.6 (#600): 教室對話目前使用的備援位置（S-03.1「備援模型」）。
-- 0 代表主模型，1–3 代表教室設定的第幾個備援；換用備援後同一場對話持續用它，不跳回主模型。
-- 只往前：新增欄位，既有對話預設 0。
ALTER TABLE conversations
    ADD COLUMN ai_fallback_position smallint NOT NULL DEFAULT 0
        CHECK (ai_fallback_position BETWEEN 0 AND 3);
