-- Seesaw 0.20 unified Store: durable queues for crash recovery.

-- Idempotent event append (required by Store trait contract).
-- Migration 016 created a non-unique index; replace with unique.
DROP INDEX IF EXISTS idx_events_id;
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_id_unique
    ON events (id) WHERE id IS NOT NULL;

-- Transient event queue
CREATE TABLE IF NOT EXISTS seesaw_events (
    row_id         BIGSERIAL    PRIMARY KEY,
    event_id       UUID         NOT NULL,
    parent_id      UUID,
    correlation_id UUID         NOT NULL,
    event_type     TEXT         NOT NULL,
    payload        JSONB        NOT NULL,
    handler_id     TEXT,
    hops           INT          NOT NULL DEFAULT 0,
    retry_count    INT          NOT NULL DEFAULT 0,
    batch_id       UUID,
    batch_index    INT,
    batch_size     INT,
    status         TEXT         NOT NULL DEFAULT 'pending',
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_seesaw_events_poll
    ON seesaw_events (correlation_id, row_id) WHERE status = 'pending';

-- Effect executions
CREATE TABLE IF NOT EXISTS seesaw_effect_executions (
    event_id            UUID         NOT NULL,
    handler_id          TEXT         NOT NULL,
    correlation_id      UUID         NOT NULL,
    event_type          TEXT         NOT NULL,
    event_payload       JSONB        NOT NULL,
    parent_event_id     UUID,
    batch_id            UUID,
    batch_index         INT,
    batch_size          INT,
    hops                INT          NOT NULL DEFAULT 0,
    attempts            INT          NOT NULL DEFAULT 0,
    max_attempts        INT          NOT NULL DEFAULT 3,
    timeout_seconds     INT          NOT NULL DEFAULT 30,
    priority            INT          NOT NULL DEFAULT 0,
    execute_at          TIMESTAMPTZ  NOT NULL DEFAULT now(),
    join_window_timeout_seconds INT,
    status              TEXT         NOT NULL DEFAULT 'pending',
    error               TEXT,
    result              JSONB,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (event_id, handler_id)
);
CREATE INDEX IF NOT EXISTS idx_seesaw_effects_poll
    ON seesaw_effect_executions (correlation_id, priority, execute_at)
    WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_seesaw_effects_running
    ON seesaw_effect_executions (updated_at)
    WHERE status = 'running';

-- Join windows
CREATE TABLE IF NOT EXISTS seesaw_join_windows (
    join_handler_id TEXT NOT NULL,
    correlation_id  UUID NOT NULL,
    batch_id        UUID NOT NULL,
    batch_size      INT  NOT NULL,
    status          TEXT NOT NULL DEFAULT 'open',
    timeout_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (join_handler_id, correlation_id, batch_id)
);

-- Join entries
CREATE TABLE IF NOT EXISTS seesaw_join_entries (
    join_handler_id   TEXT NOT NULL,
    correlation_id    UUID NOT NULL,
    batch_id          UUID NOT NULL,
    batch_index       INT  NOT NULL,
    source_event_id   UUID NOT NULL,
    event_type        TEXT NOT NULL,
    payload           JSONB NOT NULL,
    batch_size        INT  NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (join_handler_id, correlation_id, batch_id, batch_index)
);

-- Dead letter queue
CREATE TABLE IF NOT EXISTS seesaw_dead_letter_queue (
    id         BIGSERIAL PRIMARY KEY,
    event_id   UUID      NOT NULL,
    handler_id TEXT,
    error      TEXT      NOT NULL,
    reason     TEXT      NOT NULL,
    attempts   INT       NOT NULL DEFAULT 0,
    payload    JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Extend scout_runs for self-contained resume
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS task_id TEXT;
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS scope JSONB;

-- finished_at was NOT NULL DEFAULT now(), which means every INSERT gets a
-- non-null finished_at immediately. Resume needs it NULL until RunCompleted.
ALTER TABLE scout_runs ALTER COLUMN finished_at DROP NOT NULL;
ALTER TABLE scout_runs ALTER COLUMN finished_at DROP DEFAULT;
