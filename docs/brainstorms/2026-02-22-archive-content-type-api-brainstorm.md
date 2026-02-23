---
date: 2026-02-22
topic: archive-content-type-api
---

# Archive API: Trait-Based Content Types

## What We're Building

A redesign of the archive API that replaces the single `fetch(target_string)` entry point with trait-based content types. Instead of the caller encoding intent into a URL string and the router guessing what's wanted, the API exposes universal content shapes (posts, stories, short-form video, etc.) as traits. Platform-specific implementations sit behind these traits, so app code is never coupled to a specific service — only to the content type it needs.

## Why This Approach

The current `fetch()` API works for simple cases, but breaks down when a single source (like an Instagram profile) has multiple distinct content types (posts, stories, reels). URL-based routing can't express caller intent. Rather than bolting options onto the existing fetch abstraction, we're designing an API where intent is explicit and platform details are hidden.

The original reason `fetch()` existed was to cleanly map sources stored in Neo4j (as URLs) to content extraction. That mapping is preserved — archive still resolves a URL to the right implementation — but the API the caller uses is now typed and explicit.

## Key Decisions

- **Universal content type traits, not platform APIs**: App code programs against `HasPosts`, `HasStories`, `HasShortformVideo`, etc. — never against Instagram, TikTok, or Twitter directly.
- **"Content type" not "channel"**: We're describing the shape of content returned from a source. This is universal — files, search results, pages, posts, and stories are all content types under the same model.
- **Short-form video as a universal concept**: Reels, Shorts, and TikToks are the same content shape. One trait covers all.
- **SourceHandle struct, not trait objects**: `archive.source(url)` returns a concrete `SourceHandle` struct that implements all content-type traits. Returns `Err(Unsupported)` for non-applicable ones. Rust's object safety rules prevent `dyn HasPosts + HasStories`, so a single concrete type is used instead.
- **Normalized universal columns only**: Content tables store universal fields. No platform-specific columns (no retweet counts, no Reddit flair). No raw JSON escape hatch. If a platform doesn't support a field, it's null. If we later need a new field, we add a column intentionally.
- **Sources are just URLs**: The `sources` table holds an `id` and a normalized `url`. Platform and identifier are derived by the router at runtime, not stored.
- **Delete semantics extraction**: The LLM-based `resolve_semantics` step is removed.
- **`fetch()` does not survive**: The old API goes away.
- **Dynamic source handles**: `archive.source(url)` returns a uniform handle. The caller never knows the platform. Content type methods return `Vec<T>` (empty if nothing available) or `Err(Unsupported)` via `thiserror`. No `Option`, no `capabilities()` — just call what you want and handle the error.
- **Caller-driven text analysis**: Text extraction (transcription, vision/OCR, PDF parsing) is opt-in via `.with_text_analysis()`. One method — the archive picks the right extraction strategy based on `mime_type`. The app layer decides based on flags stored in Neo4j. Archive doesn't analyze unless asked.
- **run_id decoupled from archive**: Archive's job is "fetch and store content." Run-level tracking is the scout's concern, not the archive's.
- **File dedup by (url, content_hash)**: One file row per unique URL + hash. Multiple content records can reference the same file via attachments.
- **Synthetic search URLs for topic search**: Platform-wide searches (e.g., "search Instagram for coffee") use synthetic URLs like `instagram.com/explore/tags/coffee`. These are sources like anything else.
- **Big-bang migration**: All 19+ caller files updated atomically. No compatibility shim period.
- **Attachments as the media join**: Content tables don't store file references directly. An `attachments` table joins any content record to its files. One pattern for all content types — posts with carousels, stories with slides, videos with thumbnails.
- **Three decoupled layers**: Store (pure persistence, no platform knowledge), Services (platform-specific fetching, no storage knowledge), and Archive (orchestration that wires them together and exposes the trait API). Swapping a service provider doesn't touch storage. Adding a new platform doesn't change the store.
- **Files as the universal media layer**: All media (images, videos, audio, documents) lives in `files`. Other content types reference files instead of storing media URLs directly. Transcriptions and extracted text live flat on the file record — one `text` column, regardless of whether it came from a PDF parser or speech-to-text.

## Trait Surface

```rust
trait HasPosts {
    fn posts(&self, limit: u32) -> PostsRequest;
}

trait HasStories {
    fn stories(&self) -> StoriesRequest;
}

trait HasShortformVideo {
    fn short_videos(&self, limit: u32) -> ShortVideoRequest;
}

trait HasLongformVideo {
    fn videos(&self, limit: u32) -> VideoRequest;
}

trait HasTopicSearch {
    fn search_topics(&self, topics: &[&str], limit: u32) -> TopicSearchRequest;
}

trait HasPage {
    fn page(&self) -> PageRequest;
}

trait HasFile {
    fn file(&self) -> FileRequest;
}

trait HasFeed {
    fn feed(&self) -> FeedRequest;
}

trait HasSearch {
    fn search(&self, query: &str) -> SearchRequest;
}
```

## Platform → Content Type Mapping

| Platform   | Posts | Stories | Short Video | Long Video | Topic Search |
|------------|-------|---------|-------------|------------|--------------|
| Instagram  | x     | x       | x (reels)   |            | x            |
| TikTok     | x     |         | x           |            | x            |
| YouTube    |       |         | x (shorts)  | x          |              |
| Twitter/X  | x     |         |             |            | x            |
| Reddit     | x     |         |             |            | x            |
| Facebook   | x     |         |             |            |              |
| Bluesky    | x     |         |             |            | x            |

Web pages, files, feeds, and search are content types too — they just aren't platform-specific.

## Architecture: Three Layers

The archive is split into three decoupled layers:

### 1. Store — Pure persistence

Knows about sources, files, posts, stories, etc. Reads and writes to Postgres. Has no idea what Instagram is. No platform logic, no fetching logic.

### 2. Services — Platform-specific fetching

Knows how to talk to Instagram (via Apify), scrape web pages, parse PDFs, call search APIs, etc. Returns universal content types (posts, files, pages). Has no idea how storage works.

### 3. Archive — Orchestration + trait API

The public-facing layer. Resolves a source to the right service, calls the service, hands the result to the store. Exposes the trait-based API to callers.

```
Caller (scout workflow)
        ↓
    Archive (orchestration + trait API)
        ↓                ↓
    Services          Store
  (fetch data)    (persist data)
```

Benefits:
- Services are testable in isolation (mock the APIs, verify output shapes)
- Store is testable in isolation (no network calls, just Postgres)
- Swapping a service (e.g., moving from Apify to a different Instagram provider) doesn't touch storage
- Adding a new platform means adding a service + wiring it in archive — store doesn't change

## Caller API

### Getting a source handle

All sources look the same from the caller's perspective. The archive resolves a URL into a dynamic source handle:

```rust
let source = archive.source("instagram.com/starbucks").await?;
```

The caller doesn't know or care that this is Instagram. It just has a source.

### Querying content types

Content type methods return `Vec<T>` — empty if the source has nothing right now. If the source doesn't support a content type at all, it returns `Err(ArchiveError::Unsupported)` via `thiserror`:

```rust
let posts = source.posts(20).await?;          // Vec<Post> or Err(Unsupported)
let stories = source.stories().await?;         // Vec<Story> or Err(Unsupported)
let videos = source.short_videos(10).await?;   // Vec<ShortVideo> or Err(Unsupported)
```

No `Option`, no `capabilities()`. Just call what you want and handle the error. Empty vec means supported but nothing there right now.

### Transcription (caller-driven, opt-in)

Transcription is expensive. The caller explicitly requests it with a builder method:

```rust
// Without transcription — just metadata + file reference
source.short_videos(10).await?;

// With transcription — archive transcribes the video, writes text to file record
source.short_videos(10).with_transcription().await?;
```

The decision to transcribe is driven by the app layer. In practice, Neo4j source records carry a flag indicating whether transcription is desired for that source:

```rust
for neo4j_source in neo4j.get_sources_for_scout(scout_id).await? {
    let source = archive.source(&neo4j_source.url).await?;

    // Get posts — Err(Unsupported) if this source doesn't have posts
    let posts = source.posts(20).await?;
    process_posts(posts);

    // Get stories — empty vec if none right now, error if unsupported
    let stories = source.stories().await?;
    process_stories(stories);

    // Get short videos — transcribe only if flagged
    let req = source.short_videos(10);
    let videos = if neo4j_source.transcribe_videos {
        req.with_transcription().await?
    } else {
        req.await?
    };
    process_videos(videos);
}
```

### Freshness control

The caller can control caching behavior:

```rust
// Always fetch fresh from the service
source.posts(20).await?;

// Use cached if fresh enough
source.posts(20).max_age(Duration::hours(1)).await?;

// Only read from store, never fetch
source.posts(20).cached_only().await?;
```

### Non-social content types

Everything goes through the same source handle — no special cases:

```rust
let source = archive.source("https://example.com").await?;
let page = source.page().await?;           // Page or Err(Unsupported)

let source = archive.source("https://example.com/report.pdf").await?;
let file = source.file().await?;           // File or Err(Unsupported)

let source = archive.source("https://example.com/rss.xml").await?;
let feed = source.feed().await?;           // Feed or Err(Unsupported)

let source = archive.source("https://instagram.com/?q=coffee").await?;
let results = source.search().await?;      // SearchResults or Err(Unsupported)
```

A URL is a source. The caller asks for what it wants. Unsupported content types are errors via `thiserror`.

## Internal Flow

```
URL from Neo4j
        ↓
    Archive
        ↓
    resolves URL → Instagram service (via router)
        ↓
    Instagram service fetches posts → returns Vec<Post>
        ↓
    Archive hands Vec<Post> to Store
        ↓
    Store persists to posts table + files table
        ↓
    returns typed content to caller — platform-agnostic
```

## Storage Model

### sources

```
sources
  id
  url              -- normalized, canonical identifier
  created_at
```

Normalization function strips protocol, www, trailing slashes so that variant URLs resolve to the same row.

### source_content_types

Tracks freshness per content type per source.

```
source_content_types
  source_id  → sources.id
  content_type  -- "posts", "stories", "short_videos", "long_videos", "pages", "files", "feeds", "search_results"
  last_scraped_at
  unique(source_id, content_type)
```

### files

The universal media layer. All media (images, videos, audio, documents) lives here. Other content types reference files instead of storing media URLs directly.

```
files
  id
  source_id  → sources.id
  fetched_at
  content_hash
  url              -- where the file lives
  title
  mime_type        -- "video/mp4", "image/jpeg", "application/pdf", etc.
  duration         -- nullable, for audio/video
  page_count       -- nullable, for documents
  text             -- extracted/transcribed text, however we got it
  text_language    -- nullable
```

For a PDF, `text` comes from a parser. For a video, `text` comes from transcription. The caller doesn't care how it got there.

### attachments

Joins any content record to its files. One pattern for all content types — posts with carousels, stories with multiple slides, videos with thumbnails.

```
attachments
  id
  parent_type    -- "posts", "stories", "short_videos", "long_videos"
  parent_id
  file_id   → files.id
  position       -- ordering
```

### Content tables

Each content type gets its own table with universal columns. Platform-unsupported fields are null. Media is linked through the `attachments` table, not stored inline.

**posts**
```
posts
  id
  source_id  → sources.id
  fetched_at
  content_hash
  text
  location
  engagement      -- jsonb { likes, comments, shares }
  published_at
  permalink
```

**stories**
```
stories
  id
  source_id  → sources.id
  fetched_at
  content_hash
  location
  expires_at
  permalink
```

**short_videos**
```
short_videos
  id
  source_id  → sources.id
  fetched_at
  content_hash
  text
  location
  engagement      -- jsonb { likes, comments, shares }
  published_at
  permalink
```

**long_videos**
```
long_videos
  id
  source_id  → sources.id
  fetched_at
  content_hash
  text
  published_at
  permalink
```

**pages**
```
pages
  id
  source_id  → sources.id
  fetched_at
  content_hash
  markdown
  title
```

**feeds**
```
feeds
  id
  source_id  → sources.id
  fetched_at
  content_hash
  items          -- jsonb array of feed items
  title
```

**search_results**
```
search_results
  id
  source_id  → sources.id
  fetched_at
  content_hash
  query
  results        -- jsonb array of results
```

## Open Questions

- Column details per table — what we have above is a starting point. Exact fields get refined during planning.
- Migration: existing data in `web_interactions` can be dropped. Clean start.
- Freshness defaults — what's a sensible default `max_age` per content type? Stories expire fast, posts less so.
- How does the Neo4j `transcribe_videos` flag get modeled? Property on the source node, or on the relationship to the scout?

## Next Steps

→ `/workflows:plan` for implementation details
