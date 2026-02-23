-- Archive v2: Content type tables
-- Replaces the single web_interactions table with per-content-type tables.

-- Drop old table
DROP TABLE IF EXISTS web_interactions;

-- Sources: normalized URL identity
CREATE TABLE sources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_sources_url ON sources(url);

-- Source content type freshness tracking
CREATE TABLE source_content_types (
    source_id UUID NOT NULL REFERENCES sources(id),
    content_type TEXT NOT NULL,
    last_scraped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_id, content_type)
);

-- Files: universal media layer
-- All media (images, videos, audio, documents) lives here.
-- Deduped by (url, content_hash) so the same file can be referenced by multiple content records.
CREATE TABLE files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    title TEXT,
    mime_type TEXT NOT NULL,
    duration DOUBLE PRECISION,
    page_count INTEGER,
    text TEXT,
    text_language TEXT,
    UNIQUE(url, content_hash)
);
CREATE INDEX idx_files_content_hash ON files(content_hash);
CREATE INDEX idx_files_url ON files(url);

-- Attachments: polymorphic join from content records to files
CREATE TABLE attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_type TEXT NOT NULL,
    parent_id UUID NOT NULL,
    file_id UUID NOT NULL REFERENCES files(id),
    position INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_attachments_parent ON attachments(parent_type, parent_id);
CREATE INDEX idx_attachments_file ON attachments(file_id);

-- Posts (Instagram, Twitter, Reddit, Facebook, TikTok, Bluesky)
CREATE TABLE posts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    text TEXT,
    location TEXT,
    engagement JSONB,
    published_at TIMESTAMPTZ,
    permalink TEXT,
    author TEXT
);
CREATE INDEX idx_posts_source ON posts(source_id);
CREATE INDEX idx_posts_fetched ON posts(fetched_at);
CREATE INDEX idx_posts_hash ON posts(content_hash);

-- Stories (Instagram stories, etc.)
CREATE TABLE stories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    text TEXT,
    location TEXT,
    expires_at TIMESTAMPTZ,
    permalink TEXT
);
CREATE INDEX idx_stories_source ON stories(source_id);
CREATE INDEX idx_stories_fetched ON stories(fetched_at);

-- Short videos (Instagram Reels, YouTube Shorts, TikToks)
CREATE TABLE short_videos (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    text TEXT,
    location TEXT,
    engagement JSONB,
    published_at TIMESTAMPTZ,
    permalink TEXT
);
CREATE INDEX idx_short_videos_source ON short_videos(source_id);
CREATE INDEX idx_short_videos_fetched ON short_videos(fetched_at);

-- Long videos (YouTube videos, etc.)
CREATE TABLE long_videos (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    text TEXT,
    engagement JSONB,
    published_at TIMESTAMPTZ,
    permalink TEXT
);
CREATE INDEX idx_long_videos_source ON long_videos(source_id);

-- Pages (web pages)
CREATE TABLE pages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    markdown TEXT NOT NULL,
    title TEXT
);
CREATE INDEX idx_pages_source ON pages(source_id);
CREATE INDEX idx_pages_fetched ON pages(fetched_at);

-- Feeds (RSS/Atom)
CREATE TABLE feeds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    items JSONB NOT NULL,
    title TEXT
);
CREATE INDEX idx_feeds_source ON feeds(source_id);

-- Search results (web search, topic search)
CREATE TABLE search_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id),
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_hash TEXT NOT NULL,
    query TEXT NOT NULL,
    results JSONB NOT NULL
);
CREATE INDEX idx_search_results_source ON search_results(source_id);
