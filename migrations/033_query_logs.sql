-- Query logs: track what users search for to feed analytics back into the scraping pipeline.
-- Identifies gaps in coverage, prioritizes sources by demand.

CREATE TABLE query_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    member_id UUID REFERENCES members(id) ON DELETE SET NULL,
    query_text TEXT NOT NULL,
    query_type TEXT NOT NULL CHECK (query_type IN ('fts', 'semantic', 'geo', 'faceted')),
    filters JSONB NOT NULL DEFAULT '{}',
    result_count INT,
    clicked_listing_id UUID REFERENCES listings(id) ON DELETE SET NULL,
    session_id UUID,
    latitude FLOAT,
    longitude FLOAT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_query_logs_member ON query_logs(member_id);
CREATE INDEX idx_query_logs_query_text ON query_logs(query_text);
CREATE INDEX idx_query_logs_created ON query_logs(created_at DESC);
CREATE INDEX idx_query_logs_session ON query_logs(session_id);
CREATE INDEX idx_query_logs_type ON query_logs(query_type);
CREATE INDEX idx_query_logs_geo ON query_logs(latitude, longitude) WHERE latitude IS NOT NULL;
