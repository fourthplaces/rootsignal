-- Drop the foreign key on parent_id so fire-and-forget child INSERTs
-- don't race against parent INSERTs. The parent-child relationship is
-- structural metadata for tree building, not a data integrity constraint.
ALTER TABLE scout_run_events DROP CONSTRAINT scout_run_events_parent_id_fkey;
