-- Findings: the "why" layer — investigated conclusions grounded in evidence.
-- Signals are observed broadcasts. Findings are investigated conclusions.
-- Connections are edges between any two nodes with a role describing the relationship.

-- Investigation steps: ordered log of every tool call during an investigation
CREATE TABLE investigation_steps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    investigation_id UUID NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    step_number INT NOT NULL,
    tool_name TEXT NOT NULL,
    input JSONB NOT NULL DEFAULT '{}',
    output JSONB NOT NULL DEFAULT '{}',
    page_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_investigation_steps_investigation ON investigation_steps(investigation_id, step_number);

-- Findings: investigated conclusions
CREATE TABLE findings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'emerging' CHECK (status IN ('emerging', 'active', 'declining', 'resolved')),
    validation_status TEXT DEFAULT 'pending' CHECK (validation_status IN ('pending', 'validated', 'rejected')),
    signal_velocity REAL DEFAULT 0.0,
    fingerprint BYTEA NOT NULL,
    investigation_id UUID REFERENCES investigations(id) ON DELETE SET NULL,
    trigger_signal_id UUID REFERENCES signals(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(fingerprint)
);

CREATE INDEX idx_findings_status ON findings(status);
CREATE INDEX idx_findings_created ON findings(created_at DESC);
CREATE INDEX idx_findings_investigation ON findings(investigation_id);
CREATE INDEX idx_findings_trigger_signal ON findings(trigger_signal_id);

-- Full-text search on title + summary
ALTER TABLE findings ADD COLUMN search_vector tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(summary, '')), 'B')
    ) STORED;

CREATE INDEX idx_findings_search ON findings USING GIN(search_vector);

-- Connections: generic edge table linking any two nodes with a role
CREATE TABLE connections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_type TEXT NOT NULL CHECK (from_type IN ('signal', 'finding')),
    from_id UUID NOT NULL,
    to_type TEXT NOT NULL CHECK (to_type IN ('signal', 'finding')),
    to_id UUID NOT NULL,
    role TEXT NOT NULL,
    causal_quote TEXT,
    confidence REAL DEFAULT 0.7,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(from_type, from_id, to_type, to_id, role)
);

CREATE INDEX idx_connections_from ON connections(from_type, from_id);
CREATE INDEX idx_connections_to ON connections(to_type, to_id);
CREATE INDEX idx_connections_role ON connections(role);

-- Finding evidence: citations attached to a finding
CREATE TABLE finding_evidence (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    finding_id UUID NOT NULL REFERENCES findings(id) ON DELETE CASCADE,
    evidence_type TEXT NOT NULL,
    quote TEXT NOT NULL,
    attribution TEXT,
    url TEXT,
    page_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_finding_evidence_finding ON finding_evidence(finding_id);
CREATE INDEX idx_finding_evidence_type ON finding_evidence(evidence_type);

-- Add investigation flagging columns to signals
ALTER TABLE signals
    ADD COLUMN needs_investigation BOOLEAN DEFAULT FALSE,
    ADD COLUMN investigation_reason TEXT,
    ADD COLUMN investigation_status TEXT DEFAULT 'pending'
        CHECK (investigation_status IN ('pending', 'in_progress', 'completed', 'linked'));

CREATE INDEX idx_signals_needs_investigation ON signals(needs_investigation, investigation_status)
    WHERE needs_investigation = TRUE;
