-- Media and polymorphic attachments (following taggables/locationables pattern).
-- Attaches images, logos, documents, videos to any record.

CREATE TABLE media (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('image', 'logo', 'document', 'video')),
    content_type TEXT,
    alt_text TEXT,
    width INT,
    height INT,
    file_size_bytes BIGINT,
    source_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_media_type ON media(media_type);
CREATE INDEX idx_media_url ON media(url);

-- Polymorphic join for media
CREATE TABLE media_attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    media_id UUID NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    attachable_type TEXT NOT NULL,
    attachable_id UUID NOT NULL,
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    sort_order INT NOT NULL DEFAULT 0
);

CREATE INDEX idx_media_attachments_target ON media_attachments(attachable_type, attachable_id);
CREATE INDEX idx_media_attachments_target_sort ON media_attachments(attachable_type, attachable_id, sort_order);
CREATE INDEX idx_media_attachments_media ON media_attachments(media_id);
