-- F-24.1-DB (#524): only per-conversation results; topic dimensions belong to F-03.4.
-- Additive migration: preserve all deployed schemas, conversations and summaries.
CREATE FUNCTION radar_scores_valid(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT CASE WHEN jsonb_typeof(value) <> 'object' THEN false ELSE
        (SELECT count(*) = 6 AND bool_and(
            key IN ('d1', 'd2', 'd3', 'd4', 'd5', 'd6')
            AND jsonb_typeof(score) = 'number'
            AND (score #>> '{}') ~ '^[0-5]$'
        ) FROM jsonb_each(value) AS scores(key, score))
    END
$$;

CREATE TABLE conversation_radar_results (
    conversation_id uuid PRIMARY KEY REFERENCES summaries (conversation_id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'failed')),
    attempt integer NOT NULL DEFAULT 1 CHECK (attempt > 0),
    -- Stable dimension identifiers only. Names, rubrics and translations stay on the topic.
    scores jsonb,
    rules_version text,
    started_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    CONSTRAINT radar_complete_scores CHECK (scores IS NULL OR radar_scores_valid(scores)),
    CONSTRAINT radar_result_state CHECK (
        (status = 'ready' AND scores IS NOT NULL AND rules_version IS NOT NULL
            AND rules_version ~ '[^[:space:]]' AND completed_at IS NOT NULL)
        OR (status = 'pending' AND scores IS NULL AND rules_version IS NULL AND completed_at IS NULL)
        OR (status = 'failed' AND scores IS NULL AND rules_version IS NULL AND completed_at IS NOT NULL)
    )
);
