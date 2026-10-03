-- 不再有「討論活動」：教師直接建立題目，學生直接選題目開始對話。
-- 既有對話已複製題目內容，所以只移除來源欄位與活動表。

ALTER TABLE conversations DROP COLUMN activity_id;
DROP TABLE activities;
