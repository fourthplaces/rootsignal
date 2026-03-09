CREATE TABLE IF NOT EXISTS deferred_scrapes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type  TEXT NOT NULL,
    scope_data  JSONB NOT NULL,
    run_after   TIMESTAMPTZ NOT NULL,
    reason      TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_deferred_scrapes_pending
    ON deferred_scrapes (run_after)
    WHERE completed_at IS NULL;

-- Prevent duplicate deferrals for the same scope while one is still pending.
CREATE UNIQUE INDEX IF NOT EXISTS idx_deferred_scrapes_unique_pending
    ON deferred_scrapes (scope_type, scope_data)
    WHERE completed_at IS NULL;
