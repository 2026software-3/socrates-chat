-- #635: retain existing topic rubrics, scores and conversations unchanged.
-- Application creation paths explicitly opt new topics/conversations into the new rules.
ALTER TABLE topics ADD COLUMN radar_scale text NOT NULL DEFAULT 'legacy'
    CHECK (radar_scale IN ('legacy', 'stance'));
ALTER TABLE conversations
    ADD COLUMN dimension_rules boolean NOT NULL DEFAULT false,
    ADD COLUMN covered_dimensions text[] NOT NULL DEFAULT '{}';
ALTER TABLE messages ADD COLUMN question_dimensions text[] NOT NULL DEFAULT '{}';

-- Keep the legacy shape guard intact in meaning. New results allow signed integers and
-- omitted (unknown) dimensions, including an entirely unknown result {}.
ALTER TABLE conversation_radar_results DROP CONSTRAINT radar_scores_shape;
ALTER TABLE conversation_radar_results ADD CONSTRAINT radar_scores_shape CHECK (
    scores IS NULL OR (
        jsonb_typeof(scores) = 'object'
        AND NOT jsonb_path_exists(scores, 'strict $.* ? (@.type() != "number" || @ != @.floor())')
        AND CASE WHEN rules_version = 'radar-stance-v1' THEN
            NOT jsonb_path_exists(scores, 'strict $.* ? (@ < -2 || @ > 2)')
            AND (scores - ARRAY['d1','d2','d3','d4','d5','d6']) = '{}'::jsonb
        ELSE scores <> '{}'::jsonb
            AND NOT jsonb_path_exists(scores, 'strict $.* ? (@ < 0)')
        END
    )
);
