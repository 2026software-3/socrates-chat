-- F-29.4: preserve existing memberships while splitting activity-wide groups by topic.
ALTER TABLE claim_groups ADD COLUMN topic_id uuid REFERENCES topics (id);
ALTER TABLE claim_groups DROP CONSTRAINT claim_groups_activity_id_name_key;
ALTER TABLE claim_groups ADD UNIQUE (activity_id, topic_id, name);

CREATE TEMP TABLE claim_group_topic_map ON COMMIT DROP AS
SELECT pairs.old_id, pairs.topic_id, gen_random_uuid() AS new_id
FROM (
    SELECT DISTINCT g.id AS old_id, c.topic_id
    FROM claim_groups g
    JOIN summaries s ON s.claim_group_id = g.id
    JOIN conversations c ON c.id = s.conversation_id
    WHERE c.topic_id IS NOT NULL
) pairs;

INSERT INTO claim_groups (id, activity_id, topic_id, name, created_at)
SELECT m.new_id, g.activity_id, m.topic_id, g.name, g.created_at
FROM claim_group_topic_map m JOIN claim_groups g ON g.id = m.old_id;

UPDATE summaries s SET claim_group_id = m.new_id
FROM conversations c, claim_group_topic_map m
WHERE c.id = s.conversation_id AND s.claim_group_id = m.old_id AND c.topic_id = m.topic_id;

-- Legacy groups without a remaining member can be removed. A legacy member whose topic
-- was already missing keeps its original group; no topic is invented for historical data.
DELETE FROM claim_groups g WHERE g.topic_id IS NULL
AND NOT EXISTS (SELECT 1 FROM summaries s WHERE s.claim_group_id = g.id);
