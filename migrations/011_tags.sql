-- Tags (universal metadata)
CREATE TABLE tags (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    display_name TEXT,
    UNIQUE(kind, value)
);

CREATE INDEX idx_tags_kind ON tags(kind);

-- Polymorphic join for tags
CREATE TABLE taggables (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tag_id UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    taggable_type TEXT NOT NULL,
    taggable_id UUID NOT NULL,
    UNIQUE(tag_id, taggable_type, taggable_id)
);

CREATE INDEX idx_taggables_target ON taggables(taggable_type, taggable_id);
