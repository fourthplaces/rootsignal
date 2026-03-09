-- Aggregate snapshots: accelerate cold-start hydration.
-- Seesaw loads snapshot + replays remaining events instead of full replay.
-- Only the latest snapshot per aggregate is kept (upsert on save).

CREATE TABLE aggregate_snapshots (
    aggregate_type TEXT        NOT NULL,
    aggregate_id   UUID        NOT NULL,
    version        BIGINT      NOT NULL,
    state          JSONB       NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (aggregate_type, aggregate_id)
);
