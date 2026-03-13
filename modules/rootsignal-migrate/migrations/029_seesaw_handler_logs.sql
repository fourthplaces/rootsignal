CREATE TABLE IF NOT EXISTS seesaw_handler_logs (
    id              BIGSERIAL    PRIMARY KEY,
    event_id        UUID         NOT NULL,
    handler_id      TEXT         NOT NULL,
    correlation_id  UUID         NOT NULL,
    level           TEXT         NOT NULL,
    message         TEXT         NOT NULL,
    data            JSONB,
    logged_at       TIMESTAMPTZ  NOT NULL,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_handler_logs_lookup
    ON seesaw_handler_logs (event_id, handler_id);

CREATE INDEX IF NOT EXISTS idx_handler_logs_correlation
    ON seesaw_handler_logs (correlation_id);
