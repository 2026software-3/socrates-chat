-- F-24.1-DB (#524): only per-conversation results; topic dimensions belong to F-03.4.
-- Additive migration: preserve all deployed schemas, conversations and summaries.
--
-- The schema only guards the shape of `scores`: a non-empty object whose values are
-- non-negative integers. Which dimension ids are required and the score scale (S-04.2 /
-- S-04.6: d1–d6, 0–5) are enforced by `radar_store::Rubric`, so changing them never needs a
-- schema change or a function that a CHECK constraint depends on.
CREATE TABLE conversation_radar_results (
    conversation_id uuid PRIMARY KEY REFERENCES summaries (conversation_id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'failed')),
    attempt integer NOT NULL DEFAULT 1 CHECK (attempt > 0),
    -- Stable dimension identifiers only. Names, rubrics and translations stay on the topic.
    scores jsonb,
    rules_version text,
    started_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    CONSTRAINT radar_scores_shape CHECK (
        scores IS NULL OR (
            jsonb_typeof(scores) = 'object'
            AND scores <> '{}'::jsonb
            AND NOT jsonb_path_exists(
                scores,
                -- strict: lax mode would unwrap an array value such as [1] and judge its element.
                'strict $.* ? (@.type() != "number" || @ < 0 || @ != @.floor())'
            )
        )
    ),
    CONSTRAINT radar_result_state CHECK (
        (status = 'ready' AND scores IS NOT NULL AND rules_version IS NOT NULL
            AND rules_version ~ '[^[:space:]]' AND completed_at IS NOT NULL)
        OR (status = 'pending' AND scores IS NULL AND rules_version IS NULL AND completed_at IS NULL)
        OR (status = 'failed' AND scores IS NULL AND rules_version IS NULL AND completed_at IS NOT NULL)
    )
);
