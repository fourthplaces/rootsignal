-- Layer 3: Extractions (AI-structured, still raw)
CREATE TABLE extractions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    page_snapshot_id UUID NOT NULL REFERENCES page_snapshots(id) ON DELETE CASCADE,
    fingerprint BYTEA NOT NULL,
    schema_version INT NOT NULL DEFAULT 1,
    data JSONB NOT NULL,
    confidence_overall REAL NOT NULL,
    confidence_ai REAL NOT NULL,
    origin JSONB NOT NULL DEFAULT '{}',
    extracted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(fingerprint, schema_version)
);

CREATE INDEX idx_extractions_snapshot ON extractions(page_snapshot_id);
CREATE INDEX idx_extractions_extracted ON extractions(extracted_at DESC);
