-- Directed graph of organizational relationships between entities.

CREATE TABLE entity_relationships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_entity_id UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    to_entity_id UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relationship_type TEXT NOT NULL CHECK (relationship_type IN (
        'parent_of', 'partner', 'funder', 'chapter_of', 'coalition_member', 'affiliate'
    )),
    description TEXT,
    source TEXT NOT NULL CHECK (source IN ('extraction', 'manual', 'investigation')),
    confidence FLOAT,
    started_at DATE,
    ended_at DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (from_entity_id != to_entity_id),
    UNIQUE(from_entity_id, to_entity_id, relationship_type)
);

CREATE INDEX idx_entity_relationships_from ON entity_relationships(from_entity_id);
CREATE INDEX idx_entity_relationships_to ON entity_relationships(to_entity_id);
CREATE INDEX idx_entity_relationships_type ON entity_relationships(relationship_type);
CREATE INDEX idx_entity_relationships_temporal ON entity_relationships(started_at, ended_at);
