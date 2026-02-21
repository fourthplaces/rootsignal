---
date: 2026-02-21
topic: web-archive-layer
---

# Web Archive Layer

## What We're Building

A Postgres-backed web archive that is the sole interface between the scout and the internet. Scout never touches Chrome, Browserless, Serper, Apify, or RSS feeds directly — it calls `archive.fetch(target)` with a URL or query string, and the archive figures out what it is, fetches it, records everything to Postgres, and returns a typed response. A separate replay mode reads from Postgres with no network access, enabling extraction iteration, bug reproduction, and regression testing.

Everything goes through the archive. Every web interaction the scout makes is recorded.

## Why This Approach

The scout currently treats web content as ephemeral: Chrome renders a page, Readability extracts markdown, Claude extracts signals, and the original content is gone. The graph stores Evidence nodes (URL + content_hash + timestamp) but not the actual page text. This makes it impossible to re-run extraction when prompts improve, reproduce bugs from specific page content, or build regression test suites against real data.

Rather than wrapping existing scrapers with a recording layer, the archive *is* the web layer. Scout has one dependency for all web access. Concrete fetchers, content-type detection, and platform routing are all internal implementation details.

## Architecture

```
rootsignal-common    — gains shared web types (ScrapedPage, SearchResult, SocialPost, etc.)
                       plus consolidated content_hash function
rootsignal-archive   — the web layer: Archive, Replay, internal fetchers, Postgres store
rootsignal-scout     — calls archive.fetch() — no direct web access
```

### What moves where

- **Into `rootsignal-common`**: `ScrapedPage`, `SearchResult`, `SocialPost`, `SocialPlatform`, `FeedItem`, `PdfContent` (all with Serialize/Deserialize), `content_hash` (consolidated from scout duplicates)
- **Into `rootsignal-archive`**: `ChromeScraper`, `BrowserlessScraper`, `SerperSearcher`, `ApifyClient` impl for social scraping, `RssFetcher`, Readability transform logic, URL/platform detection, content-type sniffing. All private/internal.
- **Removed from `rootsignal-scout`**: `pipeline::scraper` module. Scout depends on `rootsignal-archive` and `rootsignal-common` only.

## API

### Archive (production)

```rust
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

The archive inspects the target and routes internally:

| Input | Detection | Backend | Response |
|-------|-----------|---------|----------|
| `"affordable housing Minneapolis"` | Not a URL | Serper | `Content::SearchResults` |
| `"https://city.gov/about"` | HTML page | Chrome/Browserless + Readability | `Content::Page` |
| `"https://city.gov/news.rss"` | RSS/Atom content-type | reqwest + feed-rs | `Content::Feed` |
| `"https://city.gov/report.pdf"` | PDF content-type | reqwest | `Content::Pdf` |
| `"https://instagram.com/mnfoodshelf/"` | Instagram profile URL | Apify | `Content::SocialPosts` |
| `"https://reddit.com/r/Minneapolis"` | Reddit subreddit URL | Apify | `Content::SocialPosts` |
| `"https://x.com/handle"` | Twitter URL | Apify | `Content::SocialPosts` |
| `"r/Minneapolis"` | Bare subreddit | Apify (expand to full URL) | `Content::SocialPosts` |
| Unknown content-type | Fallback | reqwest | `Content::Raw` |

Every call fetches fresh from the network. Every response is recorded to Postgres. The archive is a recording layer, not a cache.

### Response types

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
    Pdf(PdfContent),                    // PDF → extracted text + raw bytes ref
    Raw(String),                        // Anything else
}
```

### Replay (testing/iteration)

```rust
pub struct Replay { /* PgPool, optional run_id */ }

impl Replay {
    pub fn for_run(pool: PgPool, run_id: Uuid) -> Self;
    pub fn latest(pool: PgPool) -> Self;

    /// Same signature. Reads from Postgres only. No network.
    pub async fn fetch(&self, target: &str) -> Result<FetchResponse>;
    pub async fn search_social(...) -> Result<FetchResponse>;
}
```

### Shared types (in `rootsignal-common`)

```rust
pub struct ScrapedPage {
    pub url: String,
    pub raw_html: String,
    pub markdown: String,
    pub content_hash: String,
}

pub struct SearchResult { pub url: String, pub title: String, pub snippet: String }
pub struct SocialPost { pub content: String, pub author: Option<String>, pub url: Option<String> }
pub enum SocialPlatform { Instagram, Facebook, Reddit, Twitter, TikTok }
pub struct FeedItem { pub url: String, pub title: Option<String>, pub pub_date: Option<DateTime<Utc>> }
pub struct PdfContent { pub extracted_text: String }
```

### Store (internal to archive crate)

```rust
impl ArchiveStore {
    async fn latest_by_target(&self, target: &str) -> Result<Option<StoredInteraction>>;
    async fn history(&self, target: &str) -> Result<Vec<StoredInteraction>>;
    async fn by_run(&self, run_id: Uuid) -> Result<Vec<StoredInteraction>>;
    async fn by_content_hash(&self, hash: &str) -> Result<Option<StoredInteraction>>;
    async fn by_city_and_range(&self, city: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<StoredInteraction>>;
}
```

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

## Storage

**Postgres**, partitioned by time. Every interaction stored with: run_id, city_slug, detected content kind, target (normalized + raw), fetcher used, raw HTML, markdown, JSON response, raw bytes, content_hash, timestamp, duration, errors, extensible metadata.

**Retention**: everything, forever.

**Content captured per detected type**:
- Pages: raw HTML + post-Readability markdown
- RSS/Atom feeds: JSON array of feed items
- Search queries: JSON array of results
- Social posts: JSON array of posts
- PDFs: extracted text + raw bytes
- Raw/unknown: body as string

## Key Decisions

- **One primary method: `fetch(target)`** — the archive detects what the target is (URL vs query, page vs feed vs social vs PDF) and routes internally. Scout never specifies source type.
- **`search_social` is the one exception** — social topic/hashtag search requires structured input (platform + topics + limit) that can't be naturally encoded as a URL or query string. Everything else goes through `fetch()`.
- **Content enum response** — the archive tells the caller what it got back. Scout matches on the enum and handles each variant explicitly.
- **Archive is the web layer** — scout talks to archive, archive owns all fetchers, detection, and routing internally
- **Everything goes through the archive** — pages, RSS, search, social, PDFs. No exceptions.
- **No traits** — `Archive` and `Replay` are concrete structs with matching method signatures. Extract a trait later if needed (YAGNI).
- **Always fresh** — every call hits the network. Postgres history is for looking back, not for avoiding fetches.
- **Both HTML and markdown** — raw HTML for forensic/drift use cases and link extraction, markdown for signal extraction replay
- **Types in `rootsignal-common`** — not a separate crate, they're small and dependency-free
- **Postgres, everything forever** — no retention limits, no pruning

## Known Gaps

- **Binary/blob storage**: PDFs and other large binary content stored inline in Postgres for now. When this becomes a size concern, large blobs should be uploaded to object storage (S3/R2) with Postgres storing only a reference. Not a concern now — revisit when it matters.
- **PDF text extraction**: No PDF extraction library yet. Store raw bytes, add extraction later.
- **Bluesky**: Unsupported by social scraper. Archive returns error for Bluesky URLs.

## Future Directions

### LLM Response Caching / Synthesis Layer

The archive records raw web content, but the scout also makes expensive LLM calls (extraction, story weaving, response mapping) that are currently ephemeral. Ideas to explore:

- **Store extraction results alongside source content** — when you change prompts or models, compare old vs new extraction results against the same archived page. A/B testing for prompts.
- **Synthesis keys** — something like `archive.fetch(target).map(transform, "synth-key")` where derived/synthesized results are cached alongside the raw content. The archive becomes not just "what did the web say" but "what did we make of it."
- **Replay LLM outputs** — for testing downstream pipeline logic (dedup, story weaving, etc.) without burning API credits. Replay the extraction *result*, not just the source content.

Open question: is this the archive's responsibility, or a separate concern? The archive knows about web content. LLM results are a layer above. But storing them together (keyed by content_hash + prompt_version) enables powerful comparisons.

### Simulation Mode ("Mock the Web")

Beyond Replay (which plays back real recorded interactions), a third mode for running the full scout pipeline against a *constructed* world:

- **Seeded scenarios** — pre-populate the archive database with crafted content representing a specific city/situation. Run the scout against it. Verify it extracts the right signals.
- **AI-generated content on the fly** — for targets not in the seed data, generate plausible synthetic content. The archive detects "no seed for this target" and generates instead of fetching.
- **Mixed mode** — some seeded, some generated, simulating a partially-known web.
- **Embeddings for mock SERP** — simulating search results requires embeddings to make results contextually relevant to queries. The simulation layer would need to generate not just page content but also realistic search result rankings.
- **Games** — designed scenarios with known expected outcomes. "In this game, there are 3 tensions and 2 gatherings seeded across 5 pages. Did the scout find them all?" Regression testing with intent.
- **Real vs mock data separation** — clear tagging in the database so simulation data never leaks into production analysis. Possibly a separate database, or a `mode` column on every row.

This is the testing/validation endgame for the scout. Ship the core archive first, then build simulation on top of the same `fetch()` interface.

## Next Steps

-> `docs/plans/2026-02-21-feat-web-archive-layer-plan.md`
