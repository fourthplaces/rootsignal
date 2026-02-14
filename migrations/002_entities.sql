-- Layer 4: Base entity table (class table inheritance root)
CREATE TABLE entities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    description TEXT,
    website TEXT,
    phone TEXT,
    email TEXT,
    verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(name, entity_type)
);

CREATE INDEX idx_entities_type ON entities(entity_type);
CREATE INDEX idx_entities_name ON entities(name);

-- Organizations (nonprofits, community groups, faith orgs)
CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID NOT NULL UNIQUE REFERENCES entities(id) ON DELETE CASCADE,
    organization_type TEXT,
    ein TEXT,
    mission TEXT
);

-- Government entities
CREATE TABLE government_entities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID NOT NULL UNIQUE REFERENCES entities(id) ON DELETE CASCADE,
    jurisdiction TEXT,
    agency_type TEXT,
    jurisdiction_name TEXT
);

-- Business entities
CREATE TABLE business_entities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID NOT NULL UNIQUE REFERENCES entities(id) ON DELETE CASCADE,
    industry TEXT,
    is_cooperative BOOLEAN NOT NULL DEFAULT FALSE,
    is_b_corp BOOLEAN NOT NULL DEFAULT FALSE
);
