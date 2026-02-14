-- Schedules (iCal-aligned, polymorphic)
CREATE TABLE schedules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    valid_from TIMESTAMPTZ,
    valid_to TIMESTAMPTZ,
    dtstart TEXT,
    freq TEXT,
    byday TEXT,
    bymonthday TEXT,
    opens_at TIME,
    closes_at TIME,
    description TEXT,
    scheduleable_type TEXT NOT NULL,
    scheduleable_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(scheduleable_type, scheduleable_id, dtstart)
);

CREATE INDEX idx_schedules_target ON schedules(scheduleable_type, scheduleable_id);
