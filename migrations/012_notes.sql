-- Notes (annotations on any entity)
CREATE TABLE notes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'info',
    source_url TEXT,
    source_type TEXT,
    source_id UUID,
    is_public BOOLEAN NOT NULL DEFAULT FALSE,
    created_by TEXT NOT NULL DEFAULT 'system',
    expired_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_notes_severity ON notes(severity);
CREATE INDEX idx_notes_active ON notes(expired_at) WHERE expired_at IS NULL;

-- Polymorphic join for notes
CREATE TABLE noteables (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    note_id UUID NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    noteable_type TEXT NOT NULL,
    noteable_id UUID NOT NULL,
    UNIQUE(note_id, noteable_type, noteable_id)
);

CREATE INDEX idx_noteables_target ON noteables(noteable_type, noteable_id);
