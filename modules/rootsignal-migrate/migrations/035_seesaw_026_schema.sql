-- Seesaw 0.26: checkpoint model replaces event queue polling.

-- Checkpoint table: tracks per-correlation read position in event log
CREATE TABLE seesaw_checkpoints (
    correlation_id UUID PRIMARY KEY,
    position BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Handler journal: scratch storage for handlers
CREATE TABLE seesaw_handler_journal (
    handler_id TEXT NOT NULL,
    event_id UUID NOT NULL,
    seq INT NOT NULL,
    value JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (handler_id, event_id, seq)
);

-- Persistent flag on events (0.26 distinguishes persistent vs ephemeral)
ALTER TABLE events ADD COLUMN IF NOT EXISTS persistent BOOLEAN NOT NULL DEFAULT true;

-- Drop batch columns from handler executions (batch model removed in 0.26)
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS batch_id;
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS batch_index;
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS batch_size;
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS join_window_timeout_seconds;

-- Archive old event queue (checkpoint model replaces it)
ALTER TABLE IF EXISTS seesaw_events RENAME TO seesaw_events_archive;

-- Drop obsolete join tables (batch model removed in 0.26)
DROP TABLE IF EXISTS seesaw_join_entries;
DROP TABLE IF EXISTS seesaw_join_windows;

-- Backfill event_type with prefixed durable names
UPDATE events SET event_type = 'world:' || (payload->>'type')
    WHERE event_type = 'WorldEvent';
UPDATE events SET event_type = 'system:' || (payload->>'type')
    WHERE event_type = 'SystemEvent';
UPDATE events SET event_type = 'telemetry:' || (payload->>'type')
    WHERE event_type = 'TelemetryEvent';
UPDATE events SET event_type = 'scrape:' || (payload->>'type')
    WHERE event_type = 'ScrapeEvent';
UPDATE events SET event_type = 'signal:' || (payload->>'type')
    WHERE event_type = 'SignalEvent';
UPDATE events SET event_type = 'discovery:' || (payload->>'type')
    WHERE event_type = 'DiscoveryEvent';
UPDATE events SET event_type = 'lifecycle:' || (payload->>'type')
    WHERE event_type = 'LifecycleEvent';
UPDATE events SET event_type = 'enrichment:' || (payload->>'type')
    WHERE event_type = 'EnrichmentEvent';
UPDATE events SET event_type = 'expansion:' || (payload->>'type')
    WHERE event_type = 'ExpansionEvent';
UPDATE events SET event_type = 'scheduling:' || (payload->>'type')
    WHERE event_type = 'SchedulingEvent';
UPDATE events SET event_type = 'situation_weaving:' || (payload->>'type')
    WHERE event_type = 'SituationWeavingEvent';
UPDATE events SET event_type = 'supervisor:' || (payload->>'type')
    WHERE event_type = 'SupervisorEvent';
UPDATE events SET event_type = 'pipeline:' || (payload->>'type')
    WHERE event_type = 'PipelineEvent';
UPDATE events SET event_type = 'synthesis:' || (payload->>'type')
    WHERE event_type = 'SynthesisEvent';
UPDATE events SET event_type = 'curiosity:' || (payload->>'type')
    WHERE event_type = 'CuriosityEvent';
