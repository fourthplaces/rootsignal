-- Generic memoization cache for expensive function calls (LLM, etc.)
CREATE TABLE memo_cache (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    function_name TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    input_summary TEXT,
    output BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    hit_count INTEGER NOT NULL DEFAULT 0,
    UNIQUE(function_name, input_hash)
);

CREATE INDEX idx_memo_cache_expires ON memo_cache(expires_at)
WHERE expires_at IS NOT NULL;
