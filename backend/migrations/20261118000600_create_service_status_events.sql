CREATE TABLE service_status_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    service_name text NOT NULL,
    status text NOT NULL CHECK (status IN ('healthy', 'unhealthy')),
    occurred_at timestamptz NOT NULL,
    failure text CHECK (failure IN ('connection_failed', 'timeout', 'non_2xx')),
    CHECK ((status = 'unhealthy') = (failure IS NOT NULL))
);

CREATE INDEX service_status_events_service_time
    ON service_status_events (service_name, occurred_at DESC, id DESC);
CREATE INDEX service_status_events_retention ON service_status_events (occurred_at);
