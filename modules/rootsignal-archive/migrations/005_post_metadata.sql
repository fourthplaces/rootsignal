-- Richer metadata for posts and pages.

ALTER TABLE posts ADD COLUMN mentions TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE posts ADD COLUMN hashtags TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE posts ADD COLUMN media_type TEXT;
ALTER TABLE posts ADD COLUMN platform_id TEXT;

ALTER TABLE pages ADD COLUMN links TEXT[] NOT NULL DEFAULT '{}';
