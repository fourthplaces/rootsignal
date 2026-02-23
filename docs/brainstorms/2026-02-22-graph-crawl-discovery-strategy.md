---
date: 2026-02-22
topic: graph-crawl-discovery-strategy
---

# Graph-Crawl Discovery: From Tag Search to Account Crawling

## The Problem

The current discovery model leans heavily on broad searches — hashtags, keyword queries, topic discovery. This produces too much noise. A hashtag like `#MutualAidMN` returns hundreds of posts, most of which are reshares, bots, or loosely related content. The signal-to-noise ratio is poor, and the Apify/scraping budget gets burned on content that rarely produces actionable signal.

## The Shift

Stop casting wide nets. Start from known-good accounts and crawl outward.

The core insight: **if one person is good signal, the people they mention, tag, reply to, and link to are likely good signal too.** Signal clusters socially. A volunteer coordinator tags the food bank organizer. The food bank organizer links to the legal aid advocate. The chain continues. Each hop from a verified source carries trust forward.

This is a breadth-first crawl with natural pruning (backoff for accounts that don't produce), not a deep graph traversal. Each scout run expands the frontier by one level. Over time the network fills in.

## The Key Realization

**We're not building new machinery.** The existing source lifecycle — weight-based scheduling, exponential backoff, cold-tier resurrection — already handles "is this source worth continuing to scrape?" And the archive layer already extracts and stores mentions from every platform we scrape.

The strategy shift is about *how new sources enter the graph*, not how they're managed once they're there. The new work is one thing:

**Read mentions that the archive already captures → create SourceNodes from them → let the existing weight/backoff system take over.**

## What Already Exists

### Archive: Mention Extraction (done)

Every platform service in the archive already extracts mentions and persists them in `posts.mentions TEXT[]`:

| Platform | Extraction method | Source |
|----------|------------------|--------|
| Instagram | Native `p.mentions` field from Apify scraper (structured platform data) | `instagram.rs:74` |
| Twitter | `text_extract::extract_mentions()` (regex `@handle`) | `twitter.rs:60` |
| Facebook | `text_extract::extract_mentions()` | `facebook.rs:50` |
| Reddit | `text_extract::extract_mentions()` | `reddit.rs:57` |
| TikTok | `text_extract::extract_mentions()` | `tiktok.rs:56` |
| Bluesky | Rich-text facet DIDs + `text_extract::extract_mentions()` as fallback | `bluesky.rs:122-143` |

Hashtags are also extracted and stored in `posts.hashtags TEXT[]` across all platforms.

Web pages store outbound links in `pages.links TEXT[]` (migration 005) — useful for extracting social profile URLs from scraped landing pages (Linktree, org websites, etc.).

### Scout: Source Lifecycle (done)

- `SourceNode` with weight-based scheduling (0.1–1.0 drives cadence from 6h to 7d)
- Exponential backoff: 5+ consecutive empty runs → dormant
- Cold-tier resurrection: 15% of budget randomly samples dormant/never-scraped sources
- Exploration tier: 10% of budget for low-weight sources not scraped in 5+ days
- Bayesian weight computation with recency decay, diversity bonus, tension bonus

### Scout: Discovery Method Types (done)

- `DiscoveryMethod::SocialGraphFollow` — exists as enum variant, never created yet
- `DiscoveryMethod::ActorAccount` — exists for actor-linked accounts
- `MentionedAccount { platform, handle, context }` — defined in `types.rs`

### Bootstrap (done)

- `site:linktr.ee` style queries that find landing pages with social links
- Actor page discovery that extracts social links from pages
- Tension-seeded discovery queries

## What's Missing

One thing: **the scout doesn't read `posts.mentions` from the archive and promote them to SourceNodes.**

The data flows in but stops at the archive. Nobody picks it up and feeds it back into the source graph. That's the gap.

## The Model

```
Seeds (bootstrap queries + news-derived accounts)
  → Scout run (crawl known accounts → archive stores posts with mentions)
    → Promotion step reads mentions from archive → creates SourceNodes
    → New SourceNodes enter existing scheduling/weighting system
    → Repeat
```

Three seeding strategies, all producing SourceNodes that the existing machinery manages:

### 1. Bootstrap Queries (exists, keep)

Queries like `site:linktr.ee mutual aid [city]` that find landing pages with social links. Already implemented in `bootstrap.rs`.

The key change: treat bootstrap query results primarily as **account discovery**, not signal extraction. The value of a Linktree page isn't the page itself — it's the Instagram/Twitter/Facebook links on it. The `pages.links TEXT[]` column already captures these outbound links.

### 2. Recursive Account Expansion (new, core of this strategy)

After each scrape phase, read `posts.mentions` for posts scraped in this run. Each mentioned handle becomes a candidate for a new SourceNode with `discovery_method: SocialGraphFollow`. One level deep per run, recursive across runs.

The archive already does the hard work — extracting mentions from platform-native structured data (Instagram, Bluesky facets) and regex fallback (`text_extract::extract_mentions()`) across all six platforms. We just need to read it back out.

**No edge weighting.** All signal from tracked accounts is equal. An account either produces signal or it doesn't. The existing weight system handles this — signal-producing accounts rise, empty accounts decay and go dormant.

### 3. News-Based Cold Start (new, for bootstrapping new regions/topics)

For discovering accounts in domains where we have no seed graph:

```
Search news (Google News, local RSS)
  → Identify people/orgs mentioned in news articles
  → Search for their social accounts (Instagram, YouTube, Facebook, Twitter)
  → Seed those accounts into the graph as SourceNodes
```

Once accounts are seeded, strategy #2 takes over. This is the only path that's heavier (multiple search + scrape steps per account) — it only runs for cold start or periodic enrichment.

## Implementation: The Promotion Step

This is the only new pipeline step needed.

After each scrape phase completes:

1. Query the archive for `posts.mentions` from posts scraped in this run (by source_id + fetched_at window)
2. For web pages scraped in this run, also check `pages.links` for social profile URLs (instagram.com/*, twitter.com/*, etc.) — mechanical regex extraction
3. Deduplicate by `(platform, handle)` — same account mentioned by multiple sources only creates one SourceNode
4. Check if a SourceNode already exists for this handle (by canonical key)
5. If new, create `SourceNode` with:
   - `discovery_method: SocialGraphFollow`
   - `weight: 0.3` (modest starting weight — must earn its way up through the existing weight system)
   - `source_role: Mixed`
   - `gap_context`: which source(s) mentioned this account
6. New accounts get scraped on the *next* run, not the current one — avoids unbounded expansion within a single run

From here, the existing machinery takes over completely. No new scoring logic, no new scheduling, no edge weighting.

### Where This Fits in the Pipeline

```
1. Schedule (includes previously-discovered SocialGraphFollow sources — no change)
2. Phase A: Scrape → Extract → Store (archive already captures mentions — no change)
3. Account Promotion (new: read mentions from archive → create SourceNodes)
4. Mid-run Discovery (as today — no change)
5. Phase B: Scrape → Extract → Store
6. Account Promotion (again, for Phase B)
7. Synthesis, Expansion, Metrics (no change)
```

### Platform-to-Handle Mapping

The promotion step needs to know which platform a mention belongs to. This is straightforward because each post comes from a known source, and each source is tied to a platform:

- Post scraped from an Instagram source → mentions are Instagram handles
- Post scraped from a Twitter source → mentions are Twitter handles
- Post scraped from a Bluesky source → mentions are Bluesky handles (or DIDs from facets)
- etc.

Cross-platform references ("follow us on Twitter @handle" in an Instagram post) are a future enhancement — the simple version just treats mentions as same-platform.

## Hashtag Discovery: Keep as Supplement

The current `discover_from_topics` pipeline stays but its role changes. Graph crawling is the primary discovery mechanism. Hashtag/keyword discovery serves two specific purposes:

1. **Breaking events in thin-graph regions** — when a crisis hits somewhere we have no existing account graph, hashtag monitoring is the only fast-reaction mechanism. Budget should scale inversely with graph density for a region.
2. **Finding accounts outside the existing graph** — some people aren't in anyone's mention network yet. Low-budget hashtag monitoring catches them.

## SERP-Based Crawling

Some seed strategies involve SERP results (e.g., `site:linktr.ee aid`). The archive already stores `pages.links TEXT[]` for scraped web pages. The promotion step can extract social profile URLs from these links using mechanical pattern matching (instagram.com/*, twitter.com/*, facebook.com/*, youtube.com/*).

This is already partially implemented in bootstrap (actor page discovery). The promotion step generalizes it.

## Pressure Testing: Key Findings

Tested against 10 scenarios (mutual aid networks, breaking crises, hidden orgs, bot contamination, YouTube, Facebook groups, cross-platform identity, echo chambers, high-volume accounts, seasonal accounts).

**Works well for:** Mutual aid network expansion, hidden org scenarios (visible individuals layer), seasonal accounts (existing cold-tier resurrection handles it).

**Known limitations and mitigations:**

| Scenario | Limitation | Mitigation |
|----------|-----------|------------|
| Breaking crisis, no existing graph | Slow to react — graph expansion takes multiple runs | Keep hashtag discovery as supplement, increase budget for thin-graph regions |
| Bot/spam contamination | One compromised account can flood mentions | Promotion budget cap (per-run and per-source), faster backoff for SocialGraphFollow accounts |
| Facebook closed groups | Platform privacy model limits public mention graphs | Accept Facebook as weak expansion platform |
| Echo chamber / filter bubble | Graph inherits seed biases | Multiple independent seed strategies; news-based seeding cuts across social clusters |
| High-volume accounts (news stations, etc.) | Produce dozens of mentions per run, most irrelevant | Per-source promotion cap |

## Decisions

- **Promotion budget cap: required.** Both per-run cap (total new accounts) and per-source cap (no single source promotes more than N accounts). Without this, bot contamination or high-volume accounts break the system.
- **Bio scraping on first encounter: yes.** When we first scrape a new account, extract social links from their bio/profile for cross-platform expansion. Cheap, high value.
- **Faster backoff for SocialGraphFollow accounts: yes.** They haven't earned trust. 2-3 empty runs to dormant instead of 5.
- **Cross-platform identity: separate sources for now.** Merging is a hard problem with low payoff at this stage.

## Open Questions

- **Specific promotion cap numbers**: What's the right per-run and per-source cap? Needs tuning based on scrape budget.
- **News-based cold start frequency**: Run once at bootstrap? Periodically? On-demand when graph density is low?
- **Convergence requirement for promotion**: Should we require an account to be mentioned by 2+ existing sources before promoting? Reduces noise but slows expansion. Probably not for v1 — the promotion cap + faster backoff should be sufficient.

## Next Steps

→ `/workflows:plan` for implementation — the promotion step (read archive mentions → create SourceNodes) and news-based cold start
