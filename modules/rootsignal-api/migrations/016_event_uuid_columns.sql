-- Add seesaw-originated UUID causal links to the events table.
-- seq remains the replay cursor; id/parent_id carry the causal tree.

ALTER TABLE events ADD COLUMN id UUID;
ALTER TABLE events ADD COLUMN parent_id UUID;

CREATE INDEX idx_events_id ON events (id) WHERE id IS NOT NULL;
CREATE INDEX idx_events_parent_id ON events (parent_id) WHERE parent_id IS NOT NULL;
