-- F-07 / F-08：AI 判定「已符合收斂條件」（S-03.2、S-03.3）
-- 第 3 階段中學生已回應反例或對立觀點時由後端設為 true；之後 AI 提議收尾。
ALTER TABLE conversations ADD COLUMN converge_ready boolean NOT NULL DEFAULT false;
