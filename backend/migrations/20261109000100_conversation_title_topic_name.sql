-- 活動對話的標題應是題目名稱（對話開始時複製題目標題；教師的 topic 篩選與主張分組都以此區分題目）。
-- 只傳 activity_id 的舊客戶端曾把活動名稱存成標題，這裡把既有資料補成題目名稱。
--
-- 只改「標題等於所屬活動目前名稱」的活動對話，而且題目還在（topic_id 不為空）：
-- - 標題已是題目名稱、或與活動名稱不同（例如活動曾改名）的對話無法確定來源，不動；
-- - 題目已刪除（topic_id 為空）的對話沒有題目名稱可補，不動；
-- - 個人練習沒有活動，不動。
-- 重跑不會有影響：補正後標題不再等於活動名稱（兩者相同的除外，那時標題本來就是題目名稱）。
UPDATE conversations c
SET title = t.title
FROM activities a, topics t
WHERE a.id = c.activity_id
  AND t.id = c.topic_id
  AND c.title = a.title
  AND c.title <> t.title;
