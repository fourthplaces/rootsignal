-- Drop the fingerprint unique constraint (dedup is now LLM-driven).
ALTER TABLE signals DROP CONSTRAINT IF EXISTS signals_fingerprint_schema_version_key;
ALTER TABLE signals ALTER COLUMN fingerprint DROP NOT NULL;

-- Drop extraction fingerprint constraint too.
ALTER TABLE extractions DROP CONSTRAINT IF EXISTS extractions_fingerprint_schema_version_key;
ALTER TABLE extractions ALTER COLUMN fingerprint DROP NOT NULL;
