ALTER TABLE sources ADD COLUMN content_summary TEXT;

CREATE INDEX idx_embeddings_source_vector ON embeddings
    USING hnsw (embedding vector_cosine_ops)
    WHERE embeddable_type = 'source' AND locale = 'en';
