-- Phase 4: Event-backed recurring schedules.
-- Schedules are projections from ScheduleCreated/Toggled/Triggered/Deleted events.

CREATE TABLE IF NOT EXISTS schedules (
    schedule_id  TEXT        PRIMARY KEY,
    flow_type    TEXT        NOT NULL,
    scope        JSONB       NOT NULL DEFAULT '{}',
    cadence_seconds INTEGER  NOT NULL,
    enabled      BOOLEAN     NOT NULL DEFAULT true,
    last_run_id  TEXT        REFERENCES runs(run_id),
    next_run_at  TIMESTAMPTZ,
    deleted_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    region_id    TEXT
);

CREATE INDEX IF NOT EXISTS idx_schedules_due
    ON schedules(next_run_at)
    WHERE enabled = true AND deleted_at IS NULL;

-- Add FK from runs.schedule_id to schedules.
-- Clear any orphaned schedule_ids written before this table existed.
UPDATE runs SET schedule_id = NULL WHERE schedule_id IS NOT NULL;
ALTER TABLE runs ADD CONSTRAINT fk_runs_schedule_id
    FOREIGN KEY (schedule_id) REFERENCES schedules(schedule_id);
