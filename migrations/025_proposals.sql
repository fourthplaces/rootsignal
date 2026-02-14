-- Proposals: AI-generated actions that require human review before execution.
-- Any change to the entity graph (linking sources, creating entities, merging,
-- updating fields) goes through this table first.
CREATE TABLE proposals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- What this proposal targets (entity, source, listing)
    subject_type TEXT NOT NULL,
    subject_id UUID NOT NULL,

    -- What the AI wants to do
    action TEXT NOT NULL,  -- link_source, create_entity, merge_entities, update_field, unlink_source
    payload JSONB NOT NULL DEFAULT '{}',

    -- Why the AI thinks this is right
    reasoning TEXT NOT NULL,
    confidence REAL NOT NULL,
    evidence JSONB NOT NULL DEFAULT '[]',

    -- Optional link to the investigation that produced this proposal
    investigation_id UUID REFERENCES investigations(id) ON DELETE SET NULL,

    -- Review lifecycle
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, approved, rejected, auto_approved, expired
    reviewed_by TEXT,
    reviewed_at TIMESTAMPTZ,
    rejection_reason TEXT,

    -- Execution tracking
    executed_at TIMESTAMPTZ,
    execution_error TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE INDEX idx_proposals_subject ON proposals(subject_type, subject_id);
CREATE INDEX idx_proposals_status ON proposals(status);
CREATE INDEX idx_proposals_action ON proposals(action);
CREATE INDEX idx_proposals_confidence ON proposals(confidence);
CREATE INDEX idx_proposals_pending ON proposals(created_at) WHERE status = 'pending';
CREATE INDEX idx_proposals_investigation ON proposals(investigation_id);
