# Channel Weights: Per-Source Content Channel Selection

## Problem

Sources are scraped with hardcoded channel logic: web sources get `page()`, social sources get `posts()`. The archive crate supports stories, reels, feeds, and multi-channel fetch — but the scout never uses it. There's no way to say "listen for reels on this Instagram source" or "also check the RSS feed for this news site."

## Design

Every source gets a `ChannelWeights` map that controls which content channels to fetch. Weights are `0.0` (off) to `1.0` (full priority). Sources start with platform-appropriate defaults; the system promotes channels when a source proves valuable.

### Data flow

```
SourceNode.channel_weights  →  to_channels()  →  Channels (booleans)
                                                      ↓
                            ContentFetcher::fetch(source, channels)
                                                      ↓
                                              Vec<ArchiveItem>
                                                      ↓
                                    scrape domain processes each item type
```

## Changes by layer

### 1. `rootsignal-common` — types

**Add `ChannelWeights` struct** (next to existing `Channels`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelWeights {
    pub page: f64,
    pub feed: f64,
    pub media: f64,
    pub discussion: f64,
    pub events: f64,
}
```

With methods:
- `ChannelWeights::default_for(strategy: &ScrapingStrategy) -> Self` — platform-aware defaults
- `to_channels(&self) -> Channels` — threshold > 0.0 → on

Default profiles:
| Strategy | page | feed | media | discussion | events |
|---|---|---|---|---|---|
| WebPage | 1.0 | 0.0 | 0.0 | 0.0 | 0.0 |
| Rss | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| Social(Instagram) | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| Social(Reddit) | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| Social(Twitter) | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| Social(TikTok) | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| Social(Facebook) | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |
| WebQuery | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 |
| HtmlListing | 1.0 | 0.0 | 0.0 | 0.0 | 0.0 |

Media starts off by default — promoted later when the source proves valuable. `feed` is the universal "get me the latest content" channel (RSS for web, posts for social).

**Add field to `SourceNode`:**

```rust
pub channel_weights: ChannelWeights,
```

`SourceNode::new()` calls `ChannelWeights::default_for(scraping_strategy(value))`.

**Add `SourceChange` variant:**

```rust
SourceChange::ChannelWeight {
    channel: String,    // "page", "feed", "media", "discussion", "events"
    old: f64,
    new: f64,
}
```

### 2. `rootsignal-graph` — Neo4j projection

**Projector (`project_pipeline`):** On `source_discovered` and `SourcesRegistered`, write channel weights as individual properties:

```cypher
SET s.cw_page = $cw_page,
    s.cw_feed = $cw_feed,
    s.cw_media = $cw_media,
    s.cw_discussion = $cw_discussion,
    s.cw_events = $cw_events
```

On `SourceChange::ChannelWeight`, update the single property:

```cypher
MATCH (s:Source {canonical_key: $key})
SET s[$prop] = $value
```

**Reader (`row_to_source_node`):** Read `cw_*` properties back, default to platform defaults when missing (backward compat with existing sources that don't have them yet).

**Writer queries:** Add `cw_*` to all `RETURN` clauses that read SourceNodes (`get_active_sources`, `search_sources`, `get_sources_by_ids`, `source_by_id`).

### 3. `ContentFetcher` trait — add `fetch()`

Add a new method to the trait alongside the existing individual methods:

```rust
async fn fetch(&self, url: &str, channels: Channels) -> Result<Vec<ArchiveItem>>;
```

The `Archive` impl delegates to `SourceHandle::fetch(channels)` which already exists and does the right dispatch. The individual methods (`page`, `feed`, `posts`, etc.) stay for cases that need them directly (topic search, site search, url resolution).

`MockFetcher` gets a corresponding mock method. The existing per-method mocks (`on_page`, `on_posts`) can back the `fetch()` mock by decomposing `Channels` → individual calls internally.

### 4. Scrape domain — use `fetch()` with channel weights

**Key change:** The scrape handlers stop splitting sources into web/social. Instead, for each source:

1. Read `source.channel_weights.to_channels()`
2. Call `fetcher.fetch(source.value(), channels)`
3. Process `Vec<ArchiveItem>` — each variant maps to the existing extraction pipeline

This is the biggest refactor. Current shape:

```
SourcesPrepared → start_web_scrape (pages only)
               → start_social_scrape (posts only)
```

New shape:

```
SourcesPrepared → start_scrape (all sources, fetch per channel_weights)
```

The `ScrapeOutcome` / extraction pipeline stays the same — it already handles different content types. The change is in _what_ gets fetched, not how results are processed.

**Phasing this refactor:**

Phase 1 (this plan): Add `ChannelWeights` to `SourceNode`, persist to Neo4j, wire through `ContentFetcher::fetch()`. The scrape domain calls `fetch()` with the source's channels instead of hardcoded `page()`/`posts()`. Behavior is identical to today because defaults match current behavior.

Phase 2 (future): Supervisor promotes channels based on signal quality. This is a separate concern — it just writes `SourceChange::ChannelWeight` events, which the reducer and projector already handle from Phase 1.

### 5. Processing `Vec<ArchiveItem>` in the scrape domain

The scrape domain currently has separate pipelines for web content (markdown → extract) and social content (posts → combine → extract). With `fetch()` returning `Vec<ArchiveItem>`, each item type routes to the appropriate pipeline:

```rust
for item in items {
    match item {
        ArchiveItem::Page(page) => {
            // existing web_scrape pipeline: page.markdown → extractor
        }
        ArchiveItem::Feed(feed) => {
            // existing web_scrape pipeline per feed item
        }
        ArchiveItem::Posts(posts) => {
            // existing social_scrape pipeline: combine → extractor
        }
        ArchiveItem::Stories(stories) => {
            // new: similar to posts pipeline, text extraction from stories
        }
        ArchiveItem::ShortVideos(videos) => {
            // new: similar to posts pipeline, caption extraction from reels
        }
    }
}
```

Stories and ShortVideos processing is stubbed initially (the archive returns empty vecs for these anyway until Apify support lands).

## Implementation order

1. **`ChannelWeights` type + `SourceNode` field** — pure data, no behavior change
2. **Neo4j write/read** — projector writes `cw_*`, reader populates field, backward-compat defaults
3. **`SourceChange::ChannelWeight` variant** — event + projector handler
4. **`ContentFetcher::fetch()`** — new trait method + `Archive` impl + `MockFetcher`
5. **Scrape domain refactor** — replace web/social split with unified `fetch()` loop
6. **Tests** — boundary tests for channel-gated fetching

## What does NOT change

- Archive crate internals (`FetchRequest`, `Channels`, platform services) — already correct
- `ScrapingStrategy` — still used for default channel selection, not for dispatch
- Signal extraction pipeline — processes content the same way regardless of channel
- Event model — `ChannelWeights` is source metadata, not a new event domain

## Open questions

- Should `ChannelWeights` use string keys (`HashMap<String, f64>`) instead of struct fields? More extensible but loses type safety. Struct fields match `Channels` 1:1 and the set of channels is stable enough.
- Should weights influence fetch limits? e.g. `media: 0.3` → `video_limit: 3` vs `media: 1.0` → `video_limit: 10`. Deferred to Phase 2.
