ALTER TABLE web_interactions ADD COLUMN semantics JSONB;

CREATE INDEX idx_web_interactions_hash_semantics
    ON web_interactions (content_hash)
    WHERE semantics IS NOT NULL;
