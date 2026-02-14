-- Unique constraint on (name, entity_type) required for atomic upsert in Entity::find_or_create
CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_name_type ON entities(name, entity_type);
