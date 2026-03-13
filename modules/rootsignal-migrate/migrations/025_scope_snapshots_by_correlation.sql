-- Scope aggregate snapshots by correlation_id so each run has its own snapshot.
-- Previously all singleton aggregates shared Uuid::nil(), causing cross-run
-- state bleed and deserialization failures on schema evolution.

ALTER TABLE aggregate_snapshots ADD COLUMN correlation_id UUID;

-- Discard stale snapshots — they're from unknown runs and may have old schemas.
TRUNCATE aggregate_snapshots;

-- Make correlation_id NOT NULL after truncation.
ALTER TABLE aggregate_snapshots ALTER COLUMN correlation_id SET NOT NULL;

-- New primary key includes correlation_id.
ALTER TABLE aggregate_snapshots DROP CONSTRAINT aggregate_snapshots_pkey;
ALTER TABLE aggregate_snapshots ADD PRIMARY KEY (aggregate_type, aggregate_id, correlation_id);
