-- 題目取消「分類」（S-04.8 2026-11 修訂）：程式已不再讀寫 topics.category，這裡移除欄位。
-- 只往前：欄位內的既有分類資料會永久消失（需求方已決定取消分類，且分類未進入 AI 脈絡）。
ALTER TABLE topics DROP COLUMN category;
