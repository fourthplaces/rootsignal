-- Investigations: agentic runs that assess an entity/source/listing
CREATE TABLE investigations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subject_type TEXT NOT NULL,
    subject_id UUID NOT NULL,
    trigger TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    summary_confidence REAL,
    summary TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_investigations_subject ON investigations(subject_type, subject_id);
CREATE INDEX idx_investigations_status ON investigations(status);

-- Observations: structured findings about any subject
CREATE TABLE observations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subject_type TEXT NOT NULL,
    subject_id UUID NOT NULL,
    observation_type TEXT NOT NULL,
    value JSONB NOT NULL,
    source TEXT NOT NULL,
    confidence REAL NOT NULL,
    investigation_id UUID REFERENCES investigations(id) ON DELETE SET NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_observations_subject ON observations(subject_type, subject_id);
CREATE INDEX idx_observations_type ON observations(observation_type);
CREATE INDEX idx_observations_investigation ON observations(investigation_id);
