-- HNSW partial index for page_snapshot embeddings (used by qualification pre-screen)
CREATE INDEX idx_embeddings_page_snapshot_vector ON embeddings
    USING hnsw (embedding vector_cosine_ops)
    WHERE embeddable_type = 'page_snapshot' AND locale = 'en';
