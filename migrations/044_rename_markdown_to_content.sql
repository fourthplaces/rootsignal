-- Rename markdown column to content since we now store raw content (not always markdown)
ALTER TABLE page_snapshots RENAME COLUMN markdown TO content;
