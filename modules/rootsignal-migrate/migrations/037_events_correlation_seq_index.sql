-- Composite index for scoped load_from: per-engine settle loop reads
-- events by (correlation_id, seq) order. Without this, the query
-- falls back to scanning idx_events_correlation_id then sorting.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_events_correlation_seq
    ON events (correlation_id, seq)
    WHERE correlation_id IS NOT NULL;
