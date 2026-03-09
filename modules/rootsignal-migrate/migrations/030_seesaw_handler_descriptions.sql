CREATE TABLE IF NOT EXISTS seesaw_handler_descriptions (
    correlation_id  UUID  NOT NULL,
    handler_id      TEXT  NOT NULL,
    description     JSONB NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (correlation_id, handler_id)
);
