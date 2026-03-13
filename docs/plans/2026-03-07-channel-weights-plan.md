# Channel Weights: Per-Source Content Channel Selection

## Problem

Sources are scraped with hardcoded channel logic: web sources get `page()`, social sources get `posts()`. The archive crate supports stories, reels, feeds, and multi-channel fetch — but the scout never uses it. There's no way to say "listen for reels on this Instagram source" or "also check the RSS feed for this news site."

## Design

Every source gets a `ChannelWeights` map that controls which content channels to fetch. Weights are `0.0` (off) to `1.0` (full priority). Sources start with platform-appropriate defaults; the system promotes channels when a source proves valuable.

### Data flow

```
SourceNode.channel_weights
         ↓
  scrape handler reads weights
         ↓
  calls fetcher methods gated by weight > 0
  (page, feed, posts, stories, short_videos)
         ↓
  results processed through existing pipelines
```

### Key design decision: keep the web/social handler split

The web and social scrape pipelines have fundamentally different processing models:
- **Web**: per-URL fetch → content hash check → LLM extract per page
- **Social**: fetch N posts → combine into text → LLM extract per batch (Reddit sub-batches 10 at a time)

These aren't cosmetic — they're different data flow shapes. Merging them into a unified `fetch(channels)` loop would force a 300+ line function that branches on type, lose the parallelism of concurrent web/social handlers, and require changing the `SourcesPrepared` event schema (which currently only carries `web_urls`).

Channel weights gate *what* each handler fetches, not *how* results are processed. The social handler reads `source.channel_weights` and calls `posts()` (when feed > 0), `stories()` (when media > 0), `short_videos()` (when media > 0). The web handler calls `page()` (when page > 0), `feed()` (when feed > 0). No architectural change to the scrape domain.

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
- `get(&self, channel: &str) -> f64` — lookup by name (for projector/API)
- `set(&mut self, channel: &str, value: f64)` — set by name

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

Media starts off — promoted when the source proves valuable. `feed` is the universal "get me the latest content" channel (RSS for web, posts for social).

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

This follows the existing pattern: `SourceChange::Weight`, `SourceChange::Cadence`, etc. The channel is a string to keep the enum flat (one variant, not five).

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
SET s.cw_media = $value   -- dynamically constructed from channel name
```

**Reader (`row_to_source_node`):** Read `cw_*` properties back. For backward compat with existing sources that don't have them yet, fall back to `ChannelWeights::default_for(scraping_strategy(source.value()))`.

**Writer queries:** Add `cw_*` to all `RETURN` clauses that read SourceNodes (`get_active_sources`, `search_sources`, `get_sources_by_ids`, `source_by_id`).

### 3. `ContentFetcher` trait — add media methods

Add methods for the content types the scout doesn't currently fetch:

```rust
async fn stories(&self, identifier: &str) -> Result<Vec<Story>>;
async fn short_videos(&self, identifier: &str, limit: u32) -> Result<Vec<ShortVideo>>;
```

The `Archive` impl delegates to `InstagramService::fetch_stories()` / `fetch_short_videos()` which already exist as stubs.

`MockFetcher` gets corresponding `on_stories()` / `on_short_videos()` methods.

The existing individual methods (`page`, `feed`, `posts`, `search`) stay unchanged. No unified `fetch()` method — the scrape handlers already know which methods to call based on source type + channel weights.

### 4. Scrape domain — gate fetches by channel weights

**Social handler** (`social_scrape.rs`): After fetching posts (existing behavior), check `source.channel_weights.media > 0` and additionally call `stories()` / `short_videos()`. Process results through the same combine → extract pipeline that posts use.

```rust
// Existing: always fetch posts when feed channel is on
if source_channel_weights.feed > 0.0 {
    let posts = fetcher.posts(&identifier, 20).await?;
    // ... existing extraction pipeline
}

// New: fetch stories/reels when media channel is on
if source_channel_weights.media > 0.0 {
    let stories = fetcher.stories(&identifier).await.unwrap_or_default();
    let videos = fetcher.short_videos(&identifier, 10).await.unwrap_or_default();
    // ... combine captions → extract → same pipeline
}
```

**Web handler** (`web_scrape.rs`): Currently fetches pages unconditionally. Gate on `source.channel_weights.page > 0`. Could also add RSS feed fetching when `feed > 0` for web sources with known feed URLs.

**No handler restructuring needed.** Web and social handlers stay separate, run in parallel, and process results through their respective pipelines. Channel weights just expand what each handler fetches.

### 5. GraphQL API — expose channel weights

**`AdminSourceDetail` struct** (`schema.rs:1571`): Add field:

```rust
pub channel_weights: AdminChannelWeights,
```

**New GraphQL type:**

```rust
#[derive(SimpleObject)]
pub struct AdminChannelWeights {
    pub page: f64,
    pub feed: f64,
    pub media: f64,
    pub discussion: f64,
    pub events: f64,
}
```

**`source_detail()` resolver** (`schema.rs:517`): Map from `source.channel_weights`:

```rust
channel_weights: AdminChannelWeights {
    page: source.channel_weights.page,
    feed: source.channel_weights.feed,
    media: source.channel_weights.media,
    discussion: source.channel_weights.discussion,
    events: source.channel_weights.events,
},
```

### 6. Admin UI — display on source detail page

**`SOURCE_DETAIL` query** (`queries.ts:650`): Add to query:

```graphql
channelWeights {
  page
  feed
  media
  discussion
  events
}
```

**`SourceDetailPage.tsx`**: Add a "Channels" card in the metadata grid (alongside Weight, Scrape Stats, Schedule, Output). Display each channel as a labeled weight with visual indication of on/off:

```tsx
<div className="rounded-lg border border-border p-4 space-y-3">
  <h3 className="text-sm font-medium text-muted-foreground">Channels</h3>
  <dl className="grid grid-cols-5 gap-4">
    {["page", "feed", "media", "discussion", "events"].map((ch) => {
      const w = source.channelWeights[ch];
      return (
        <MetaCard
          key={ch}
          label={ch}
          value={w > 0 ? w.toFixed(1) : "off"}
        />
      );
    })}
  </dl>
</div>
```

Channels with weight `0` show "off" in muted text. Channels with weight > 0 show the numeric weight. This gives immediate visibility into what the system is listening for.

## Phase 2: Dynamic channel promotion (future)

Phase 1 is purely structural — add the field, wire it through, display it. Behavior is identical to today because defaults match current hardcoded logic.

Phase 2 adds the intelligence: the supervisor observes signal quality per source and promotes/demotes channels.

### Where promotion decisions live

The **supervisor domain** already adjusts source properties reactively:
- `apply_source_penalties()` computes `quality_penalty` from validation issues
- `reset_resolved_penalties()` resets penalties when issues are resolved
- Scheduling recomputes `weight` and `cadence` from signal production history

Channel promotion follows the same pattern. The supervisor would emit `SourceChange::ChannelWeight` events based on rules like:

- Source consistently produces high-confidence signals → promote `media` to 0.5
- Source's media channel produces noise for N consecutive runs → demote back to 0.0
- Source corroborates signals from multiple other sources → promote `discussion`

### How promotion triggers work

The supervisor already has access to Neo4j (signal quality, corroboration) and Postgres (run history, validation issues). It can query:

```
For each source with media == 0:
  - Has it produced N+ high-confidence signals in the last M runs?
  - Is quality_penalty >= 0.8 (no validation issues)?
  - Has it been active for K+ runs?
If all true → emit SourceChange::ChannelWeight { channel: "media", old: 0.0, new: 0.5 }
```

### Weight as priority signal

Beyond on/off gating, weights could influence fetch aggressiveness:
- `media: 0.3` → `video_limit: 3` (tentative, small sample)
- `media: 1.0` → `video_limit: 10` (proven, full fetch)

This is a simple `(weight * max_limit).ceil() as u32` calculation in the scrape handler. Deferred until we have data showing it matters.

### Decay and demotion

Channels that stop producing should be demoted. The supervisor could run periodic checks:
- Media channel on for N runs with 0 signals from media content → demote to 0.0
- Gradual decay: reduce weight by 0.1 per empty run rather than hard cut

All of this is domain logic in the supervisor — the infrastructure from Phase 1 (events, projection, UI) supports it without changes.

## Implementation order

1. **`ChannelWeights` type + `SourceNode` field** — pure data, no behavior change
2. **Neo4j write/read** — projector writes `cw_*`, reader populates field, backward-compat defaults
3. **`SourceChange::ChannelWeight` variant** — event + projector handler
4. **GraphQL API** — `AdminChannelWeights` type, `source_detail` resolver, query
5. **Admin UI** — Channels card on `SourceDetailPage`
6. **`ContentFetcher` trait** — add `stories()`, `short_videos()` methods
7. **Scrape domain** — gate fetches by channel weights
8. **Tests** — boundary tests for channel-gated fetching

Steps 1–5 are purely structural (data + display). Steps 6–7 are behavioral (actually fetching more content). Step 8 validates the behavior.

## What does NOT change

- Archive crate internals (`FetchRequest`, `Channels`, platform services) — already correct
- Web/social handler split — stays, reflects real processing differences
- `ScrapingStrategy` — still used for default channel selection
- Signal extraction pipeline — processes content the same way regardless of channel
- `SourcesPrepared` event schema — no changes needed
- Completion gates (`tension_web_done` / `tension_social_done`) — unchanged
