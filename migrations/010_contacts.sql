-- Contacts (polymorphic)
CREATE TABLE contacts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT,
    title TEXT,
    email TEXT,
    phone TEXT,
    department TEXT,
    contactable_type TEXT NOT NULL,
    contactable_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(contactable_type, contactable_id, email)
);

CREATE INDEX idx_contacts_target ON contacts(contactable_type, contactable_id);
