-- F-32.1: 題目的引導問題（S-04.8）
-- 題目可附 0 到 5 個引導問題，每個最多 200 字；題目庫為全域，預設為空陣列。
ALTER TABLE topics
    ADD COLUMN guiding_questions text[] NOT NULL DEFAULT ARRAY[]::text[],
    ADD CONSTRAINT topics_guiding_questions_count_check CHECK (coalesce(cardinality(guiding_questions), 0) <= 5);

