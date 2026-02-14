-- Add review_status to observations for admin moderation queue
ALTER TABLE observations ADD COLUMN review_status TEXT NOT NULL DEFAULT 'pending';

CREATE INDEX idx_observations_review_status ON observations (review_status);
