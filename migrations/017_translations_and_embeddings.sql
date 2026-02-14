-- Multi-language support: translations, embeddings, and source_locale tracking

-- Translations (polymorphic, per-field)
CREATE TABLE translations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    translatable_type TEXT NOT NULL,
    translatable_id UUID NOT NULL,
    field_name TEXT NOT NULL,
    locale TEXT NOT NULL,
    content TEXT NOT NULL,
    source_locale TEXT,
    translated_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(translatable_type, translatable_id, field_name, locale)
);

CREATE INDEX idx_translations_target ON translations(translatable_type, translatable_id);
CREATE INDEX idx_translations_locale ON translations(locale);
CREATE INDEX idx_translations_lookup ON translations(translatable_type, translatable_id, locale);

-- Embeddings (polymorphic, per-record per-locale)
CREATE TABLE embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    embeddable_type TEXT NOT NULL,
    embeddable_id UUID NOT NULL,
    locale TEXT NOT NULL DEFAULT 'en',
    embedding VECTOR(1536) NOT NULL,
    source_text_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(embeddable_type, embeddable_id, locale)
);

CREATE INDEX idx_embeddings_target ON embeddings(embeddable_type, embeddable_id);
CREATE INDEX idx_embeddings_locale ON embeddings(locale);

-- HNSW index for fast vector similarity search (English embeddings)
CREATE INDEX idx_embeddings_vector ON embeddings
    USING hnsw (embedding vector_cosine_ops)
    WHERE locale = 'en';

-- Add source_locale to content tables
ALTER TABLE listings ADD COLUMN source_locale TEXT NOT NULL DEFAULT 'en';
ALTER TABLE entities ADD COLUMN source_locale TEXT NOT NULL DEFAULT 'en';
ALTER TABLE services ADD COLUMN source_locale TEXT NOT NULL DEFAULT 'en';

-- Remove embedding from listings (moved to embeddings table)
ALTER TABLE listings DROP COLUMN IF EXISTS embedding;
