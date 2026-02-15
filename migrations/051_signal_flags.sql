CREATE TABLE signal_flags (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    signal_id UUID NOT NULL REFERENCES signals(id) ON DELETE CASCADE,
    flag_type TEXT NOT NULL CHECK (flag_type IN ('wrong_type', 'wrong_entity', 'expired', 'spam')),
    suggested_type TEXT CHECK (suggested_type IN ('ask', 'give', 'event', 'informative')),
    comment TEXT,
    resolved BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_signal_flags_signal ON signal_flags(signal_id);
CREATE INDEX idx_signal_flags_unresolved ON signal_flags(resolved) WHERE resolved = FALSE;
