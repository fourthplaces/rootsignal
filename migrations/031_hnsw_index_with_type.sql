-- Narrower HNSW partial index scoped to listing embeddings in English
DROP INDEX IF EXISTS idx_embeddings_vector;
CREATE INDEX idx_embeddings_vector ON embeddings
    USING hnsw (embedding vector_cosine_ops)
    WHERE embeddable_type = 'listing' AND locale = 'en';
