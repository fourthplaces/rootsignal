---
title: "feat: Web Archive Layer"
type: feat
date: 2026-02-21
---

# Web Archive Layer

## Overview

Build `rootsignal-archive`, a Postgres-backed web archive that becomes the sole interface between the scout and the internet. One primary method — `archive.fetch(target)` — accepts a URL or query string, detects what it is, fetches it through the appropriate backend, records everything to Postgres, and returns a typed `Content` enum. A `Replay` struct with the same interface reads from Postgres with no network access.

Brainstorm: `docs/brainstorms/2026-02-21-web-archive-brainstorm.md`

## Problem Statement

The scout treats web content as ephemeral. Chrome renders a page, Readability extracts markdown, Claude extracts signals, and the original content is gone. The graph stores Evidence nodes (URL + content_hash + timestamp) but not the actual page text. This makes it impossible to:

- Re-run extraction when prompts/models improve
- Reproduce bugs from specific page content
- Build regression test suites against real data
- Detect content drift or suppression over time

## Proposed Solution

A new crate `rootsignal-archive` that owns all web fetching infrastructure. Chrome, Browserless, Serper, Apify, and RSS fetching move inside as private implementation details. The scout calls `archive.fetch()` and matches on the returned `Content` enum.

### API Design

```rust
// rootsignal-archive/src/archive.rs

pub struct Archive { /* PgPool, fetchers, run_id, city_slug */ }

impl Archive {
    pub fn new(pool: PgPool, config: ArchiveConfig, run_id: Uuid, city_slug: String) -> Self;

    /// The primary entry point. Pass a URL or query string.
    /// The archive detects what it is, fetches it, records it, returns typed content.
    pub async fn fetch(&self, target: &str) -> Result<FetchResponse>;

    /// Social topic/hashtag search — requires structured input that can't be
    /// encoded as a single URL or query string.
    pub async fn search_social(
        &self,
        platform: &SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<FetchResponse>;
}
```

**Why `search_social` is separate**: The `discover_from_topics` flow passes a platform enum plus a list of topic strings to search across. There's no natural single-string encoding for "search Instagram for hashtags ['minneapolisHousing', 'mnFoodShelf'] with limit 30". Rather than invent a synthetic URL scheme, this one operation gets a dedicated method. Everything else — pages, RSS, search queries, social profile URLs — goes through `fetch()`.

### Response Types

```rust
pub struct FetchResponse {
    pub target: String,
    pub content: Content,
    pub content_hash: String,
    pub fetched_at: DateTime<Utc>,
    pub duration_ms: u32,
}

pub enum Content {
    Page(ScrapedPage),                  // HTML page → raw_html + markdown
    Feed(Vec<FeedItem>),                // RSS/Atom → parsed items
    SearchResults(Vec<SearchResult>),   // Web search → structured results
    SocialPosts(Vec<SocialPost>),       // Social feed/search → posts
    Pdf(PdfContent),                    // PDF → extracted text (+ raw bytes ref)
    Raw(String),                        // Anything else
}
```

`ScrapedPage` contains both `raw_html` and `markdown`, which solves the `HtmlListing` problem — the caller can use `raw_html` for link extraction and `markdown` for signal extraction, from a single fetch.

### Routing Logic

The archive inspects the target and routes:

| Input | Detection | Internal Backend | Content variant |
|-------|-----------|-----------------|-----------------|
| `"affordable housing Minneapolis"` | Not a URL (`is_web_query()`) | Serper | `SearchResults` |
| `"https://city.gov/about"` | HTTP + `text/html` | Chrome/Browserless + Readability | `Page` |
| `"https://city.gov/news.rss"` | HTTP + `application/rss+xml` or `application/atom+xml` | reqwest + feed-rs | `Feed` |
| `"https://city.gov/report.pdf"` | HTTP + `application/pdf` or `.pdf` extension | reqwest | `Pdf` |
| `"https://instagram.com/mnfoodshelf/"` | Instagram profile URL pattern | Apify | `SocialPosts` |
| `"https://reddit.com/r/Minneapolis"` | Reddit subreddit URL pattern | Apify | `SocialPosts` |
| `"https://x.com/handle"` or `"https://twitter.com/handle"` | Twitter URL pattern | Apify | `SocialPosts` |
| `"https://tiktok.com/@user"` | TikTok URL pattern | Apify | `SocialPosts` |
| `"r/Minneapolis"` | Bare subreddit reference | Apify (expand to full URL) | `SocialPosts` |
| Unknown content-type | Fallback | reqwest | `Raw` |

**Content-type detection strategy**: For URLs, first check for social platform URL patterns (cheap, no HTTP needed). If not social, do a HEAD request to check Content-Type. If ambiguous (e.g. `text/xml` that could be RSS), fetch the body and sniff (feed-rs can detect).

**Default limits**: Baked into the archive based on platform: Reddit=20, other social=10, Serper=5. The `search_social` method takes an explicit limit for discovery flows that need 30.

### Replay

```rust
pub struct Replay { /* PgPool, optional run_id */ }

impl Replay {
    pub fn for_run(pool: PgPool, run_id: Uuid) -> Self;
    pub fn latest(pool: PgPool) -> Self;

    /// Same signature. Reads from Postgres only. No network.
    /// Returns Err if no archived record exists for this target.
    pub async fn fetch(&self, target: &str) -> Result<FetchResponse>;
    pub async fn search_social(...) -> Result<FetchResponse>;
}
```

**Lookup key**: The normalized target string (via `sanitize_url` for URLs, trimmed for queries). Must use the same normalization in both Archive (recording) and Replay (lookup).

**Missing records**: `Replay.fetch()` returns `Err(ArchiveError::NotFound)` — distinct from network errors. Callers can distinguish "never fetched" from "fetched but failed".

## Technical Approach

### Postgres Schema

```sql
-- migrations/001_create_web_interactions.sql

CREATE TABLE web_interactions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id          UUID NOT NULL,
    city_slug       TEXT NOT NULL,
    kind            TEXT NOT NULL,    -- 'page', 'feed', 'search', 'social', 'pdf', 'raw'
    target          TEXT NOT NULL,    -- normalized URL or query string (lookup key)
    target_raw      TEXT NOT NULL,    -- original target as passed to fetch()
    fetcher         TEXT NOT NULL,    -- 'chrome', 'browserless', 'serper', 'apify', 'reqwest'
    raw_html        TEXT,             -- page scrapes only
    markdown        TEXT,             -- page scrapes only (post-Readability)
    response_json   JSONB,            -- search/social/feed results
    raw_bytes       BYTEA,            -- PDFs and binary content
    content_hash    TEXT NOT NULL,    -- FNV-1a hash (same algo as Evidence nodes)
    fetched_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_ms     INTEGER NOT NULL,
    error           TEXT,             -- null on success, error message on failure
    metadata        JSONB             -- extensible: platform, limit, topics, etc.
) PARTITION BY RANGE (fetched_at);

-- Initial partition (monthly)
CREATE TABLE web_interactions_2026_02 PARTITION OF web_interactions
    FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');

-- Lookup indexes
CREATE INDEX idx_web_interactions_target ON web_interactions (target, fetched_at DESC);
CREATE INDEX idx_web_interactions_run ON web_interactions (run_id);
CREATE INDEX idx_web_interactions_hash ON web_interactions (content_hash);
CREATE INDEX idx_web_interactions_city_time ON web_interactions (city_slug, fetched_at DESC);
```

**Key decisions**:
- `target` is the normalized lookup key; `target_raw` preserves the original input
- Social/search/feed results stored as JSONB in `response_json`
- Failed fetches are recorded with `error` populated (so Replay can reproduce failures too)
- `metadata` JSONB for extensible fields (platform name, limit used, topic list, etc.)
- Monthly partitions for query performance; new partitions created by a migration or at startup

### Crate Structure

```
modules/rootsignal-archive/
├── Cargo.toml
├── migrations/
│   └── 001_create_web_interactions.sql
└── src/
    ├── lib.rs          -- pub mod + re-exports
    ├── error.rs        -- ArchiveError enum + Result<T>
    ├── archive.rs      -- Archive struct (production, always-fetch + record)
    ├── replay.rs       -- Replay struct (Postgres-only, no network)
    ├── store.rs        -- ArchiveStore (Postgres persistence, crate-private)
    ├── router.rs       -- Target detection: URL pattern matching, content-type sniffing
    ├── readability.rs  -- HTML → markdown transform (wraps spider_transformations)
    └── fetchers/
        ├── mod.rs
        ├── page.rs     -- Chrome + Browserless (moved from scout)
        ├── search.rs   -- Serper (moved from scout)
        ├── social.rs   -- Apify social scraping (moved from scout)
        └── feed.rs     -- RSS/Atom fetcher (moved from scout)
```

### Shared Types in rootsignal-common

Add to `modules/rootsignal-common/src/types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedPage {
    pub url: String,
    pub raw_html: String,
    pub markdown: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialPost {
    pub content: String,
    pub author: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    pub url: String,
    pub title: Option<String>,
    pub pub_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfContent {
    pub extracted_text: String,
}
```

Also consolidate `content_hash` into `rootsignal-common` (currently in `scout::infra::util`).

Also add `Serialize + Deserialize` to the existing `SocialPlatform` enum in common, and remove the duplicate local enum from `scraper.rs`.

### Configuration

```rust
pub struct ArchiveConfig {
    pub page_backend: PageBackend,
    pub serper_api_key: String,
    pub apify_api_key: Option<String>,
}

pub enum PageBackend {
    Chrome,
    Browserless { base_url: String, token: Option<String> },
}
```

Constructed from environment variables in `scout::main.rs`, same as today but passed to `Archive::new()` instead of constructing scrapers directly.

## Implementation Phases

### Phase 1: Foundation — crate scaffolding + shared types

**Goal**: New crate compiles, shared types in common, workspace wired up.

- Create `modules/rootsignal-archive/` with `Cargo.toml` and `src/lib.rs`
- Add to workspace `members` and `[workspace.dependencies]` in root `Cargo.toml`
- Add `sqlx` to workspace dependencies: `sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres", "chrono", "uuid", "json"] }`
- Add shared types to `rootsignal-common/src/types.rs`: `ScrapedPage`, `SearchResult`, `SocialPost`, `FeedItem`, `PdfContent`
- Add `Serialize + Deserialize` to existing `SocialPlatform` in common
- Move `content_hash` from `scout::infra::util` to `rootsignal-common`
- Update scout to import `content_hash` from common (both `util.rs` and `investigator.rs`)
- Create `error.rs` with `ArchiveError` enum
- Create skeleton `archive.rs`, `replay.rs`, `store.rs`, `router.rs`
- Verify `cargo check --workspace` passes

**Files touched**:
- `Cargo.toml` (workspace root)
- `modules/rootsignal-archive/Cargo.toml` (new)
- `modules/rootsignal-archive/src/lib.rs` (new)
- `modules/rootsignal-archive/src/error.rs` (new)
- `modules/rootsignal-archive/src/archive.rs` (new, skeleton)
- `modules/rootsignal-archive/src/replay.rs` (new, skeleton)
- `modules/rootsignal-archive/src/store.rs` (new, skeleton)
- `modules/rootsignal-archive/src/router.rs` (new, skeleton)
- `modules/rootsignal-common/src/types.rs` (add shared web types)
- `modules/rootsignal-common/src/lib.rs` (re-export new types + content_hash)
- `modules/rootsignal-scout/src/infra/util.rs` (content_hash → import from common)
- `modules/rootsignal-scout/src/enrichment/investigator.rs` (content_hash → import from common)
- `modules/rootsignal-scout/Cargo.toml` (will gain rootsignal-archive dep later)

### Phase 2: Router — target detection logic

**Goal**: Given a target string, determine what kind of content it is and which fetcher to use.

- Implement `router.rs`: `detect_target(target: &str) -> TargetKind`
- `TargetKind` enum: `WebQuery`, `SocialProfile { platform, identifier }`, `Url` (needs HTTP to determine content type)
- URL pattern matching for social platforms (Instagram, Reddit, Twitter/X, TikTok, Facebook)
- Handle edge cases: bare `r/subreddit`, `twitter.com` vs `x.com`, Instagram post vs profile vs hashtag
- Reuse `is_web_query()` from common for query detection
- Post-HTTP content-type routing: HTML → Page, RSS/Atom → Feed, PDF → Pdf, unknown → Raw
- RSS sniffing fallback: if content-type is ambiguous (`text/xml`), try feed-rs parse
- Unit tests for all URL patterns and edge cases

**Files touched**:
- `modules/rootsignal-archive/src/router.rs`

### Phase 3: Fetchers — move scraping infrastructure into archive

**Goal**: All fetching backends live inside the archive crate.

- Move `ChromeScraper` from `scout::pipeline::scraper` → `archive::fetchers::page`
- Move `BrowserlessScraper` from `scout::pipeline::scraper` → `archive::fetchers::page`
- Move `SerperSearcher` from `scout::pipeline::scraper` → `archive::fetchers::search`
- Move `ApifyClient` social scraper impl from `scout::pipeline::scraper` → `archive::fetchers::social`
- Move `RssFetcher` from `scout::pipeline::scraper` → `archive::fetchers::feed`
- Move Readability transform logic → `archive::readability`
- Move `extract_links_by_pattern` → `archive` (or keep in common since it's a pure function)
- Move `sanitize_topics_to_hashtags` → `archive::fetchers::social`
- Update import types to use shared types from common (`SearchResult`, `SocialPost`, etc.)
- Each fetcher returns the common types, not internal structs
- `PageFetcher` returns `ScrapedPage` (both raw HTML + markdown from single fetch)
- All fetchers are `pub(crate)` — not exposed outside the archive

**Files touched**:
- `modules/rootsignal-archive/src/fetchers/mod.rs` (new)
- `modules/rootsignal-archive/src/fetchers/page.rs` (new, from scout)
- `modules/rootsignal-archive/src/fetchers/search.rs` (new, from scout)
- `modules/rootsignal-archive/src/fetchers/social.rs` (new, from scout)
- `modules/rootsignal-archive/src/fetchers/feed.rs` (new, from scout)
- `modules/rootsignal-archive/src/readability.rs` (new, from scout)
- `modules/rootsignal-archive/Cargo.toml` (add deps: browserless-client, apify-client, spider_transformations, feed-rs, reqwest, url, tempfile, rand)

### Phase 4: Store — Postgres persistence

**Goal**: Record and retrieve web interactions in Postgres.

- Write SQL migration: `migrations/001_create_web_interactions.sql`
- Implement `ArchiveStore` with sqlx:
  - `insert(interaction)` — record a fetch result
  - `latest_by_target(target)` — most recent interaction for a normalized target
  - `history(target)` — all snapshots of a target over time
  - `by_run(run_id)` — everything from a specific run
  - `by_content_hash(hash)` — lookup by Evidence-compatible hash
  - `by_city_and_range(city, from, to)` — city + time range query
- Run migrations at startup via `sqlx::migrate!()`
- Use `sqlx::PgPool` for connection pooling (consistent with how Neo4j uses `deadpool`)
- Store social/search/feed results as JSONB (serialize via serde)
- Failed fetches recorded with `error` column populated

**Files touched**:
- `modules/rootsignal-archive/migrations/001_create_web_interactions.sql` (new)
- `modules/rootsignal-archive/src/store.rs`

### Phase 5: Archive + Replay — the public API

**Goal**: `Archive::fetch()` and `Replay::fetch()` work end-to-end.

- Implement `Archive::new()` — construct fetchers from config, wrap PgPool
- Implement `Archive::fetch(target)`:
  1. Route via `router::detect_target(target)`
  2. For social URLs: dispatch to `fetchers::social`
  3. For queries: dispatch to `fetchers::search`
  4. For URLs: HTTP HEAD → content-type → dispatch to appropriate fetcher
  5. Record result to Postgres via `store.insert()`
  6. Return `FetchResponse` with appropriate `Content` variant
- Implement `Archive::search_social(platform, topics, limit)`
- Implement `Replay::fetch(target)` — query store, map to `FetchResponse`
- Implement `Replay::search_social(...)` — query store by metadata
- Error handling: Postgres write failure logs a warning but still returns `Ok(content)` — don't fail the scrape because the archive write failed
- Archive must be `Send + Sync` — callers drive concurrency via `buffer_unordered`
- Preserve Chrome semaphore (`MAX_CONCURRENT_CHROME = 2`) inside page fetcher

**Files touched**:
- `modules/rootsignal-archive/src/archive.rs`
- `modules/rootsignal-archive/src/replay.rs`
- `modules/rootsignal-archive/src/lib.rs` (re-exports)

### Phase 6: Scout migration — wire archive into scout

**Goal**: Scout uses `Archive` for all web access. `pipeline::scraper` module removed.

- In `scout::main.rs`: construct `Archive` from env vars + PgPool, pass to `Scout::new()`
- Change `Scout` to hold `Arc<Archive>` instead of separate scraper/searcher/social fields
- Change `ScrapePhase` to accept `&Archive` instead of three separate trait objects
- Migrate `run_web()`:
  - Web queries: `archive.fetch(query_str)` → match on `Content::SearchResults`
  - RSS feeds: `archive.fetch(feed_url)` → match on `Content::Feed`
  - HTML listings: `archive.fetch(url)` → match on `Content::Page`, use `raw_html` for link extraction
  - Page scrapes: `archive.fetch(url)` → match on `Content::Page`, use `markdown` for extraction
- Migrate `run_social()`:
  - Social profiles: `archive.fetch(source_url)` → match on `Content::SocialPosts`
- Migrate `discover_from_topics()`:
  - Topic search: `archive.search_social(platform, topics, limit)` → match on `Content::SocialPosts`
  - Site-scoped search: `archive.fetch(query)` then `archive.fetch(result_url)`
- Migrate enrichment modules (`investigator.rs`, `tension_linker.rs`, `response_finder.rs`, `gathering_finder.rs`) — these currently receive `Arc<dyn PageScraper>`, change to `Arc<Archive>`
- Remove `pipeline::scraper` module from scout
- Remove direct dependencies on `apify-client`, `browserless-client`, `spider_transformations`, `feed-rs` from scout's `Cargo.toml`
- Add `rootsignal-archive` dependency to scout's `Cargo.toml`
- Verify `cargo check --workspace` passes
- Run existing tests, fix breakages

**Files touched**:
- `modules/rootsignal-scout/src/main.rs`
- `modules/rootsignal-scout/src/scout.rs`
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs`
- `modules/rootsignal-scout/src/pipeline/mod.rs`
- `modules/rootsignal-scout/src/enrichment/investigator.rs`
- `modules/rootsignal-scout/src/discovery/tension_linker.rs`
- `modules/rootsignal-scout/src/discovery/response_finder.rs`
- `modules/rootsignal-scout/src/discovery/gathering_finder.rs`
- `modules/rootsignal-scout/src/pipeline/scraper.rs` (deleted)
- `modules/rootsignal-scout/Cargo.toml`

### Phase 7: Tests

**Goal**: Archive has integration tests, scout tests pass with archive.

- Integration tests for `Archive::fetch()` with each content type (using `simweb` or testcontainers)
- Integration tests for `Replay::fetch()` round-trip (fetch → store → replay → compare)
- Unit tests for router (URL pattern detection)
- Unit tests for store (insert + query)
- Verify existing scout tests still pass
- Consider testcontainers for Postgres (already in workspace deps)

**Files touched**:
- `modules/rootsignal-archive/tests/` (new)

## Acceptance Criteria

### Functional Requirements

- [x] `archive.fetch(url)` fetches a web page and returns `Content::Page` with both `raw_html` and `markdown`
- [x] `archive.fetch(query)` runs a Serper search and returns `Content::SearchResults`
- [x] `archive.fetch(rss_url)` fetches and parses an RSS feed, returns `Content::Feed`
- [x] `archive.fetch(social_url)` detects platform and returns `Content::SocialPosts`
- [x] `archive.search_social(platform, topics, limit)` searches social platform by topics
- [x] Every `fetch()` call records the interaction to Postgres
- [x] Failed fetches are recorded with error information
- [x] `Replay::fetch(target)` returns the most recent archived content for that target
- [x] `Replay::fetch(target)` returns `Err(NotFound)` for unarchived targets
- [ ] Scout produces identical signals before and after the migration (same extraction, same dedup)

### Non-Functional Requirements

- [x] Archive is `Send + Sync` — callers can drive concurrency via `buffer_unordered`
- [x] Chrome concurrency limit (`MAX_CONCURRENT_CHROME = 2`) preserved
- [x] Postgres write failure does not fail the fetch — logs warning, returns content
- [x] `cargo check --workspace` passes with no warnings
- [x] No direct web access from scout — all fetching goes through archive (via bridge traits)

## Dependencies & Risks

**New dependencies**:
- `sqlx` (Postgres driver) — first Postgres usage in the project, adds build-time and runtime dependency
- All existing web deps (`browserless-client`, `apify-client`, `spider_transformations`, `feed-rs`) move from scout to archive

**Risks**:
- **Migration scope**: Touching `scrape_phase.rs` is high-risk — it's the core pipeline. The phase 6 migration must be done carefully, preferably one call site at a time.
- **Content-type sniffing**: Some sites serve RSS as `text/html` or PDFs as `application/octet-stream`. The router needs fallback detection.
- **Social URL patterns**: Platform URLs change over time (twitter.com → x.com). The router must handle legacy and current domains.
- **Postgres availability**: Scout currently runs with only Neo4j. Adding Postgres as a hard requirement changes deployment. Consider making archive optional (feature flag) for initial rollout.

## Known Gaps

- **Binary/blob storage**: PDFs and large binary content stored inline in Postgres for now. When this becomes a size concern, large blobs should be uploaded to object storage (S3/R2) with Postgres storing only a reference.
- **PDF text extraction**: No PDF extraction library in the project yet. Phase 1 can store raw PDF bytes and return `Content::Pdf` with empty extracted text. Add a library (e.g. `pdf-extract`) when needed.
- **Bluesky**: Currently unsupported by the social scraper. Archive should return `Err(UnsupportedPlatform)` for Bluesky URLs until support is added.
- **Run-level replay**: Replaying a full scout run in order requires capturing the orchestration sequence. Existing run log JSON files capture this, but wiring it up is deferred.
- **Partition management**: Monthly Postgres partitions need to be created ahead of time. Could be automated at startup or via a cron job. Punt for now — create a few months of partitions in the migration.
- **Within-run dedup**: The archive does not deduplicate within a run. Caller (`scrape_phase.rs`) remains responsible for URL dedup before fetching, same as today.
- **Blocked URL filtering**: Stays in the caller (`scrape_phase.rs`). The archive has no awareness of the graph's blocked URL list.

## References

- Brainstorm: `docs/brainstorms/2026-02-21-web-archive-brainstorm.md`
- Prior brainstorm: `docs/brainstorms/2026-02-18-web-archive-brainstorm.md`
- Current scraper implementation: `modules/rootsignal-scout/src/pipeline/scraper.rs`
- Current scrape pipeline: `modules/rootsignal-scout/src/pipeline/scrape_phase.rs`
- Scout construction: `modules/rootsignal-scout/src/scout.rs:128-169`
- Content hash: `modules/rootsignal-scout/src/infra/util.rs:26`
- Workspace Cargo.toml: `Cargo.toml`
