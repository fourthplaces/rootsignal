---
title: "feat: Social Discovery — LLM-Generated Topics Feed Multi-Platform Search"
type: feat
date: 2026-02-19
---

# Social Discovery: LLM-Generated Topics Feed Multi-Platform Search

## Overview

The scout's discovery engine generates web search queries to fill gaps in the tension landscape. But the most important signals for active community tensions don't live on the web — they live on social media, posted by individuals who are publicly visible while the organizations they protect stay hidden.

We extend the discovery engine to generate **social discovery topics** (hashtags + search terms) alongside web queries. These feed into platform-specific pipelines that search Instagram, X/Twitter, TikTok, and GoFundMe. When discovery finds someone posting community signals, we auto-follow them as a Source so they get scraped on future runs.

**Brainstorm:** `docs/brainstorms/2026-02-19-social-discovery-brainstorm.md`

## Problem Statement

From the volunteer coordinator interview: organizations helping with immigration enforcement fear in Minnesota deliberately hide from public visibility. Coordination happens via private group chats, word of mouth, and text. The organizations are invisible to web search.

But individuals step forward publicly. The volunteer posts every day on Instagram. GoFundMe campaigns are created under personal names to protect the churches. The signal is on social media — posted by people who are willing to be visible so the organizations don't have to be.

The scout currently only generates `WebQuery` sources (web search). The `discover_from_topics()` pipeline is fully built but permanently stubbed — `let topics: Vec<String> = Vec::new()`. The Apify scrapers for Instagram hashtags, X/Twitter, TikTok, and GoFundMe all exist in `apify-client` but aren't wired into discovery.

## Proposed Solution

### Architecture

```
Discovery LLM call (existing)
  ├── queries: Vec<DiscoveryQuery>        ← web search (existing)
  └── social_topics: Vec<SocialTopic>     ← NEW: hashtags + search terms
                │
                ▼
    ┌─────────────────────────────────┐
    │   discover_from_topics()        │  ← currently stubbed, gets populated
    │   (per-platform dispatch)       │
    └─────────┬───────────────────────┘
              │
    ┌─────────┼──────────┬──────────────┐
    ▼         ▼          ▼              ▼
Instagram  X/Twitter   TikTok      GoFundMe
hashtag    keyword     keyword     keyword
search     search      search      search
    │         │          │              │
    ▼         ▼          ▼              ▼
 Posts →   Tweets →   Captions →   Campaigns →
 LLM ext   LLM ext   LLM ext     LLM extraction
    │         │          │              │
    ├─────────┼──────────┼──────────────┤
    ▼                                   ▼
 Auto-follow                     Signals directly
 productive                      (campaigns are
 accounts as                      one-shot)
 Sources
```

### Data Flow

The `SourceDiscoverer::run()` method currently returns `DiscoveryStats`. We extend it to also return social topics. These get passed to `discover_from_topics()` in the scout's main `run()` loop — simple in-memory threading, no graph persistence needed for topics themselves.

```
SourceDiscoverer::run() → (DiscoveryStats, Vec<SocialTopic>)
                                              │
Scout::run() passes topics to ──────────────► discover_from_topics(topics)
```

## Technical Approach

### Step 1: Extend discovery LLM output — `SocialTopic` struct

**File:** `modules/rootsignal-scout/src/discovery.rs`

Add to `DiscoveryPlan`:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SocialTopic {
    /// A hashtag or search term (plain text, no # prefix)
    pub topic: String,
    /// Why this topic — what gap it fills
    pub reasoning: String,
}

pub struct DiscoveryPlan {
    #[serde(default, deserialize_with = "deserialize_queries")]
    pub queries: Vec<DiscoveryQuery>,
    #[serde(default)]
    pub social_topics: Vec<SocialTopic>,  // NEW
}
```

`#[serde(default)]` means if the LLM returns only `queries` (or the field is missing/malformed), social topics silently become an empty vec. Web query generation quality is never degraded.

### Step 2: Update discovery system prompt

**File:** `modules/rootsignal-scout/src/discovery.rs`

Extend `discovery_system_prompt()` to add social topic instructions after the web query section:

```
Additionally, generate 2-5 social media discovery topics — hashtags and
search terms for finding individuals posting publicly about community
tensions on social media (Instagram, X/Twitter, TikTok, GoFundMe).

Social topics should target:
- Individuals publicly advocating, volunteering, or organizing
- GoFundMe campaigns for community causes
- Hashtags used by local advocacy communities
- Search terms for mutual aid, donations, volunteer coordination

Include the city name or state abbreviation. Examples:
- "MNimmigration" (Instagram hashtag)
- "sanctuary city Minneapolis volunteer" (X/Twitter keyword)
- "immigration legal aid Minnesota" (GoFundMe search)

Focus on PEOPLE, not organizations — individuals who chose to be
publicly visible. Organizations may be deliberately hidden.
```

### Step 3: Return topics from `SourceDiscoverer`

**File:** `modules/rootsignal-scout/src/discovery.rs`

Change `SourceDiscoverer::run()` signature:

```rust
pub async fn run(&self) -> (DiscoveryStats, Vec<String>) {
    // ... existing logic ...
    // After discover_from_curiosity, collect social_topics from plan
    (stats, social_topics)
}
```

In `discover_from_curiosity()`, after parsing the `DiscoveryPlan`, extract `social_topics` and return them alongside stats. The topics are plain strings — `SocialTopic.topic` values.

### Step 4: Add keyword search methods to `apify-client`

**File:** `modules/apify-client/src/lib.rs` + `types.rs`

The X/Twitter and TikTok scrapers currently only support profile-based scraping. We need keyword search variants. Two options:

**Option A: Use different Apify actors for keyword search.**
- X/Twitter: `apidojo/tweet-scraper` supports a `searchTerms` field (need to verify actor API)
- TikTok: `clockworks/tiktok-scraper` may support a `searchQueries` field (need to verify)

**Option B: Use Twitter Advanced Search and TikTok search URLs as startUrls.**

For now, plan for adding `searchTerms`/`searchQueries` fields to existing input structs. If the actors don't support it, we'll find alternative actors.

New methods:

```rust
// apify-client/src/lib.rs
pub async fn search_x_posts(&self, keywords: &[&str], limit: u32) -> Result<Vec<Tweet>>
pub async fn search_tiktok_posts(&self, keywords: &[&str], limit: u32) -> Result<Vec<TikTokPost>>
// scrape_gofundme already supports keyword search ✓
// search_instagram_hashtags already works ✓
```

New/updated input types:

```rust
// apify-client/src/types.rs
pub struct TweetSearchInput {
    #[serde(rename = "searchTerms")]
    pub search_terms: Vec<String>,
    #[serde(rename = "maxItems")]
    pub max_items: u32,
}

pub struct TikTokSearchInput {
    #[serde(rename = "searchQueries")]
    pub search_queries: Vec<String>,
    #[serde(rename = "resultsPerPage")]
    pub results_per_page: u32,
}
```

Add `into_discovered()` converters to `Tweet` and `TikTokPost` (matching `InstagramPost::into_discovered()`):

```rust
impl Tweet {
    pub fn into_discovered(self) -> Option<DiscoveredPost> {
        let content = self.content()?.to_string();
        let author_username = self.author.as_ref()?.user_name.clone()?;
        Some(DiscoveredPost {
            content,
            author_display_name: self.author.as_ref().and_then(|a| a.name.clone()),
            author_username,
            post_url: self.url.unwrap_or_default(),
            timestamp: None, // parse created_at if needed
            platform: "x".to_string(),
        })
    }
}
```

### Step 5: Generalize `SocialScraper::search_hashtags` → `search_topics`

**File:** `modules/rootsignal-scout/src/scraper.rs`

The current trait method:
```rust
async fn search_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<SocialPost>>;
```

**Rename + generalize:**
```rust
async fn search_topics(
    &self,
    platform: SocialPlatform,
    topics: &[&str],
    limit: u32,
) -> Result<Vec<SocialPost>>;
```

Extend `SocialPlatform`:
```rust
pub enum SocialPlatform {
    Instagram,
    Facebook,
    Reddit,
    Twitter,   // NEW
    TikTok,    // NEW
}
```

The `ApifyClient` impl dispatches by platform:
- `Instagram` → `search_instagram_hashtags` (existing)
- `Twitter` → `search_x_posts` (new)
- `TikTok` → `search_tiktok_posts` (new)
- `Facebook` / `Reddit` → return empty (no keyword search for these)

Update `NoopSocialScraper` and test mocks to implement the new signature.

### Step 6: Add GoFundMe discovery pipeline

**File:** `modules/rootsignal-scout/src/scout.rs`

GoFundMe campaigns have structured data rich enough for direct signal extraction via LLM. The flow:

1. Call `self.apify.scrape_gofundme(topic, limit)` for each social topic
2. Concatenate campaign title + description as "content"
3. Feed through `self.extractor.extract()` (same as other platforms)
4. Store signals through normal pipeline
5. **No auto-follow** — campaigns are one-shot signals, not recurring sources

This runs alongside the other platform searches in `discover_from_topics()`.

**Why LLM extraction instead of direct conversion:** The existing pipeline (geo-filter, dedup, quality scoring, embedding) expects `Node` objects from the extractor. Bypassing the extractor means building a parallel pipeline. LLM extraction on campaign text is cheap (Haiku) and gives us signal type classification for free.

### Step 7: Populate `discover_from_topics()` — the actual wiring

**File:** `modules/rootsignal-scout/src/scout.rs`

This is the core change. Replace the stub:

```rust
// WAS:
let topics: Vec<String> = Vec::new();

// NOW: topics come from discovery LLM (passed as parameter)
```

Change `discover_from_topics` signature to accept topics:

```rust
async fn discover_from_topics(
    &self,
    topics: &[String],         // NEW: from discovery LLM
    stats: &mut ScoutStats,
    embed_cache: &mut EmbeddingCache,
    source_signal_counts: &mut HashMap<String, u32>,
    known_city_urls: &HashSet<String>,
)
```

In `Scout::run()`, thread the topics from discovery to topic search:

```rust
// Mid-run discovery
let (discovery_stats, social_topics) = discoverer.run().await;

// Topic discovery — search social media for community signals
self.discover_from_topics(&social_topics, &mut stats, &mut embed_cache, ...).await;
```

**Multi-platform dispatch inside `discover_from_topics`:**

```rust
// Search each platform with the same topics
let platforms = [
    SocialPlatform::Instagram,
    SocialPlatform::Twitter,
    SocialPlatform::TikTok,
];

for platform in &platforms {
    let posts = self.social.search_topics(*platform, &topic_strs, POSTS_PER_SEARCH).await;
    // ... existing group-by-author, extract, auto-follow logic ...
    // Use correct SourceType when creating Source nodes
}

// GoFundMe: separate path (campaigns, not accounts)
for topic in topics.iter().take(MAX_GOFUNDME_SEARCHES) {
    let campaigns = self.apify.scrape_gofundme(topic, CAMPAIGNS_PER_SEARCH).await;
    // ... extract signals from campaign descriptions ...
}
```

**Platform-aware source creation:** Replace the hardcoded `SourceType::Instagram` with the correct type based on platform:

```rust
let (source_type, source_url) = match platform {
    SocialPlatform::Instagram => (SourceType::Instagram, format!("https://www.instagram.com/{username}/")),
    SocialPlatform::Twitter => (SourceType::Twitter, format!("https://x.com/{username}")),
    SocialPlatform::TikTok => (SourceType::TikTok, format!("https://www.tiktok.com/@{username}")),
    _ => continue,
};
```

### Step 8: Wire new platform sources into `scrape_social_media`

**File:** `modules/rootsignal-scout/src/scout.rs`

The existing `scrape_social_media()` method (Phase B) loads sources from the graph and scrapes them. It currently handles Instagram, Facebook, Reddit. Add arms for Twitter and TikTok:

```rust
match source.source_type {
    SourceType::Instagram => { /* existing */ }
    SourceType::Facebook => { /* existing */ }
    SourceType::Reddit => { /* existing */ }
    SourceType::Twitter => { /* NEW: scrape_x_posts via SocialScraper */ }
    SourceType::TikTok => { /* NEW: scrape_tiktok_posts via SocialScraper */ }
    _ => continue,
}
```

This ensures that auto-followed accounts from discovery actually get scraped on subsequent runs.

## Acceptance Criteria

- [ ] Discovery LLM generates 2-5 social topics alongside web queries
- [ ] `discover_from_topics()` receives LLM-generated topics (no longer stubbed)
- [ ] Instagram hashtag search works with LLM-generated topics (existing pipeline, now populated)
- [ ] X/Twitter keyword search discovers tweets and auto-follows productive accounts
- [ ] TikTok keyword search discovers posts and auto-follows productive accounts
- [ ] GoFundMe keyword search discovers campaigns and extracts signals (no auto-follow)
- [ ] Auto-followed accounts use correct `SourceType` (Twitter, TikTok, not hardcoded Instagram)
- [ ] Auto-followed accounts get scraped on subsequent runs via `scrape_social_media()`
- [ ] `SocialPlatform` enum includes `Twitter` and `TikTok` variants
- [ ] All new code compiles clean with `cargo check --workspace`
- [ ] Existing tests pass: `cargo test -p rootsignal-scout`

## Budget and Cost Control

**Constants:**
```rust
const MAX_SOCIAL_TOPICS: usize = 5;          // Topics from LLM per run
const MAX_SOCIAL_SEARCHES: usize = 3;         // Platform searches per run (across all platforms)
const MAX_GOFUNDME_SEARCHES: usize = 2;       // GoFundMe searches per run
const POSTS_PER_SEARCH: u32 = 20;             // Posts per platform search (existing)
const CAMPAIGNS_PER_SEARCH: u32 = 10;         // GoFundMe campaigns per search
const MAX_NEW_ACCOUNTS: usize = 5;            // New accounts to follow per run (existing)
```

**Cost model per run (worst case):**
- 3 platform searches × 1 Apify call = 3 calls (6 cents)
- 2 GoFundMe searches × 1 Apify call = 2 calls (4 cents)
- LLM extraction: ~5 Haiku calls for discovered content (~2 cents)
- Total marginal cost: ~12 cents/run

**No additional LLM call for topic generation** — topics come from the same discovery LLM call that generates web queries.

## Dependencies & Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| X/Twitter Apify actor may not support keyword search | Blocks Twitter discovery | Verify actor API before implementing; fall back to different actor |
| TikTok Apify actor may not support keyword search | Blocks TikTok discovery | Verify actor API; TikTok is lowest priority platform |
| LLM generates poor social topics | Low signal-to-noise | Same feedback loop as web queries — track performance, LLM sees what worked |
| Apify rate limiting across platforms | All social discovery fails | Sequential platform calls, budget cap, graceful error handling per platform |
| Social posts lack geo data → filtered out | Valid signals rejected | Treat topic-discovered sources as city-local (topic itself was city-scoped) |
| GoFundMe campaigns may be national, not local | Off-geography signals | Geo-filter still applies; campaign `location` field helps |

## Implementation Order

1. **`SocialTopic` struct + `DiscoveryPlan` extension** — add field + serde default
2. **Discovery system prompt** — add social topic instructions
3. **`SourceDiscoverer::run()` return type** — return topics alongside stats
4. **`SocialPlatform` enum extension** — add `Twitter`, `TikTok`
5. **Apify client search methods** — `search_x_posts`, `search_tiktok_posts` keyword variants
6. **`SocialScraper` trait generalization** — `search_hashtags` → `search_topics`
7. **`discover_from_topics()` activation** — accept topics param, multi-platform dispatch
8. **GoFundMe discovery pipeline** — campaigns → LLM extraction → signals
9. **`scrape_social_media()` extension** — handle Twitter/TikTok source types
10. **Scout `run()` wiring** — thread topics from discovery to topic search

## Pre-Implementation: Verify Apify Actor APIs

Before starting Step 5, verify:
- Does `apidojo/tweet-scraper` (actor `61RPP7dywgiy0JPD0`) support `searchTerms` input?
- Does `clockworks/tiktok-scraper` (actor `GdWCkxBtKWOsKjdch`) support `searchQueries` input?

If not, identify alternative Apify actors for keyword search on these platforms.

## References

- Brainstorm: `docs/brainstorms/2026-02-19-social-discovery-brainstorm.md`
- Interview context: `docs/interviews/2026-02-17-volunteer-coordinator-interview.md`
- Scout pipeline: `docs/architecture/scout-pipeline.md`
- Discovery engine: `modules/rootsignal-scout/src/discovery.rs`
- Topic discovery stub: `modules/rootsignal-scout/src/scout.rs:1060`
- SocialScraper trait: `modules/rootsignal-scout/src/scraper.rs:362`
- Apify client: `modules/apify-client/src/lib.rs`
- Apify types: `modules/apify-client/src/types.rs`
