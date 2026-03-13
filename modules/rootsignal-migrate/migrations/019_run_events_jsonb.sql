-- Replace 39 type-specific columns with a single JSONB data column.
-- The event_type column is kept for easy filtering.

ALTER TABLE scout_run_events ADD COLUMN data JSONB;

-- Drop the 39 type-specific columns
ALTER TABLE scout_run_events
    DROP COLUMN source_url,
    DROP COLUMN query,
    DROP COLUMN url,
    DROP COLUMN provider,
    DROP COLUMN platform,
    DROP COLUMN identifier,
    DROP COLUMN signal_type,
    DROP COLUMN title,
    DROP COLUMN result_count,
    DROP COLUMN post_count,
    DROP COLUMN items,
    DROP COLUMN content_bytes,
    DROP COLUMN content_chars,
    DROP COLUMN signals_extracted,
    DROP COLUMN implied_queries,
    DROP COLUMN similarity,
    DROP COLUMN confidence,
    DROP COLUMN success,
    DROP COLUMN action,
    DROP COLUMN node_id,
    DROP COLUMN matched_id,
    DROP COLUMN existing_id,
    DROP COLUMN new_source_url,
    DROP COLUMN canonical_key,
    DROP COLUMN gatherings,
    DROP COLUMN needs,
    DROP COLUMN stale,
    DROP COLUMN sources_created,
    DROP COLUMN spent_cents,
    DROP COLUMN remaining_cents,
    DROP COLUMN topics,
    DROP COLUMN posts_found,
    DROP COLUMN reason,
    DROP COLUMN strategy,
    DROP COLUMN field,
    DROP COLUMN old_value,
    DROP COLUMN new_value,
    DROP COLUMN signal_count,
    DROP COLUMN summary;

-- Index for JSONB queries on common fields
CREATE INDEX idx_sre_data_node_id ON scout_run_events ((data->>'node_id')) WHERE data->>'node_id' IS NOT NULL;
