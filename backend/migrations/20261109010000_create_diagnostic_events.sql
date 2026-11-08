CREATE TABLE diagnostic_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    request_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('ai', 'login', 'account_lock')),
    failure_class text CHECK (
        failure_class IS NULL OR
        (kind = 'ai' AND failure_class IN ('connection_failed', 'timeout', 'invalid_key',
            'quota_exceeded', 'rate_limited', 'service_error')) OR
        (kind = 'login' AND failure_class IN ('invalid_credentials', 'account_disabled', 'account_locked')) OR
        (kind = 'account_lock' AND failure_class = 'account_locked')
    ),
    function text NOT NULL,
    route text NOT NULL,
    status smallint NOT NULL CHECK (status BETWEEN 100 AND 599),
    error_code text,
    -- Deliberately no FK: retained security events survive account or classroom removal.
    user_id uuid,
    classroom_id uuid,
    duration_ms bigint CHECK (duration_ms >= 0),
    retry_count integer CHECK (retry_count >= 0),
    model text,
    CHECK ((failure_class IS NULL) = (error_code IS NULL)),
    CHECK (kind <> 'account_lock' OR failure_class IS NOT NULL),
    CHECK ((kind = 'ai') = (duration_ms IS NOT NULL AND retry_count IS NOT NULL))
);

CREATE INDEX diagnostic_events_time_idx ON diagnostic_events (occurred_at DESC, id DESC);
CREATE INDEX diagnostic_events_request_idx ON diagnostic_events (request_id, occurred_at DESC);
CREATE INDEX diagnostic_events_user_idx ON diagnostic_events (user_id, occurred_at DESC) WHERE user_id IS NOT NULL;
CREATE INDEX diagnostic_events_classroom_idx ON diagnostic_events (classroom_id, occurred_at DESC) WHERE classroom_id IS NOT NULL;
