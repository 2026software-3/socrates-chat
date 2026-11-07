-- 先建立新的唯一索引，再移除舊索引；sqlx migrate 在同一交易中執行。
-- 舊規則每位學生每個活動至多一場，因此既有資料符合新的逐題唯一規則。
CREATE UNIQUE INDEX conversations_user_activity_topic_uniq
    ON conversations (user_id, activity_id, topic_id) WHERE activity_id IS NOT NULL;

DROP INDEX conversations_user_activity_uniq;

-- 保留原索引名稱，讓滾動部署期間舊版和新版後端都能識別唯一性衝突。
ALTER INDEX conversations_user_activity_topic_uniq RENAME TO conversations_user_activity_uniq;
