-- S-17.3：寄信的「開始時間」。重寄用它判斷同一則客訴是不是已經有寄信在進行（避免連點寄出兩封），
-- 啟動時也用它找出重啟或寫回失敗而永遠停在 pending 的客訴並重新排入寄信。
-- 既有列為空值：舊的 pending 視為已逾時，啟動時會重寄一次。
ALTER TABLE complaints ADD COLUMN mail_send_started_at timestamptz;
