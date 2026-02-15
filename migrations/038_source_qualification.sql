ALTER TABLE sources ADD COLUMN qualification_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE sources ADD COLUMN qualification_summary TEXT;
ALTER TABLE sources ADD COLUMN qualification_score INTEGER;
