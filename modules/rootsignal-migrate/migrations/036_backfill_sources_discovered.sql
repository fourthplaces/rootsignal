-- Backfill sources_discovered field on source_discovered event payloads.
-- Older events predate this field; the projector requires it on SourceNode.

UPDATE events
SET payload = jsonb_set(payload, '{source,sources_discovered}', '0')
WHERE event_type IN ('pipeline:source_discovered', 'discovery:source_discovered')
  AND NOT (payload->'source' ? 'sources_discovered');

-- Remove orphaned scout_task events. These event types were removed from
-- the SystemEvent enum and have no producer or consumer.

DELETE FROM events
WHERE event_type IN (
    'system:scout_task_created',
    'system:scout_task_cancelled',
    'system:task_phase_transitioned',
    'system:source_registered'
);
