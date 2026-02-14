-- Services (persistent capabilities, HSDS-aligned)
CREATE TABLE services (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    url TEXT,
    email TEXT,
    phone TEXT,
    interpretation_services TEXT,
    application_process TEXT,
    fees_description TEXT,
    eligibility_description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_services_entity ON services(entity_id);
CREATE INDEX idx_services_status ON services(status);
