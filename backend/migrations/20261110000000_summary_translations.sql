-- The source-language version stays in summaries. Each other language has its own result or pending job.
CREATE TABLE summary_translations (
    conversation_id uuid NOT NULL REFERENCES summaries (conversation_id) ON DELETE CASCADE,
    language text NOT NULL CHECK (language IN ('zh-TW', 'en', 'es')),
    stance text,
    reasons text,
    attempt_token uuid,
    available_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (conversation_id, language),
    CHECK ((stance IS NULL AND reasons IS NULL) OR
           (stance IS NOT NULL AND reasons IS NOT NULL AND btrim(stance) <> '' AND btrim(reasons) <> ''))
);

CREATE INDEX summary_translations_pending_idx
    ON summary_translations (available_at, conversation_id, language) WHERE stance IS NULL;
