-- F-07 / F-08：AI 回覆的階段判斷（S-03.2、S-03.3）

-- 第 3 階段中學生已回應反例或對立觀點時由後端設為 true；之後 AI 提議收尾。
ALTER TABLE conversations ADD COLUMN converge_ready boolean NOT NULL DEFAULT false;

-- 正在產生 AI 回覆的時間；同一場對話同時間只允許一條回覆串流。
-- 超過逾時上限仍未清除（例如服務重啟）視為過期，可重新產生。
ALTER TABLE conversations ADD COLUMN generating_since timestamptz;

-- 模型對「是否達到進入下一階段條件」的判斷理由，供稽核與評估，不回傳給學生
ALTER TABLE messages ADD COLUMN advance_reason text;
