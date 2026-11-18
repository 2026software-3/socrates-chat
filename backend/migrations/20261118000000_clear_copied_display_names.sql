-- display_name 只存使用者自訂的名稱（#548）；舊版寫入路徑把匯入姓名複製進去，
-- 這裡清掉與匯入姓名完全相同的複本，讓顯示改由 COALESCE(display_name, imported_name) 解析。
UPDATE users
SET display_name = NULL
WHERE imported_name IS NOT NULL
  AND display_name = imported_name;
