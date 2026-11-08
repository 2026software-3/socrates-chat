ALTER TABLE topics
    ADD COLUMN dimensions jsonb,
    ADD COLUMN dimensions_prompt_version text,
    ADD COLUMN dimensions_pending_languages text[] NOT NULL DEFAULT '{}',
    ADD COLUMN has_had_conversation boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT topic_dimensions_complete CHECK (
        (dimensions IS NULL AND dimensions_prompt_version IS NULL)
        OR (
            dimensions IS NOT NULL AND dimensions_prompt_version IS NOT NULL
            AND jsonb_typeof(dimensions) = 'array'
            AND CASE WHEN jsonb_typeof(dimensions) = 'array' THEN
                jsonb_array_length(dimensions) = 6
                AND (dimensions->0->>'id') IS NOT DISTINCT FROM 'd1'
                AND (dimensions->1->>'id') IS NOT DISTINCT FROM 'd2'
                AND (dimensions->2->>'id') IS NOT DISTINCT FROM 'd3'
                AND (dimensions->3->>'id') IS NOT DISTINCT FROM 'd4'
                AND (dimensions->4->>'id') IS NOT DISTINCT FROM 'd5'
                AND (dimensions->5->>'id') IS NOT DISTINCT FROM 'd6'
            ELSE false END
        )
    ),
    ADD CONSTRAINT topic_dimensions_pending_languages CHECK (
        dimensions_pending_languages <@ ARRAY['zh-TW', 'en', 'es']
    );

UPDATE topics SET has_had_conversation = true
WHERE id IN (SELECT DISTINCT topic_id FROM conversations WHERE topic_id IS NOT NULL);

CREATE FUNCTION lock_topic_dimensions_on_conversation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE topics SET has_had_conversation = true
    WHERE id = NEW.topic_id AND NOT has_had_conversation;
    RETURN NEW;
END;
$$;

CREATE TRIGGER lock_topic_dimensions_on_conversation
AFTER INSERT OR UPDATE OF topic_id ON conversations
FOR EACH ROW WHEN (NEW.topic_id IS NOT NULL)
EXECUTE FUNCTION lock_topic_dimensions_on_conversation();
