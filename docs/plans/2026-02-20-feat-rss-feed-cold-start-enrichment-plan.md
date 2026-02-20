---
title: "feat: RSS Feed Cold Start Enrichment"
type: feat
date: 2026-02-20
---

# feat: RSS Feed Cold Start Enrichment

## Overview

Add local news RSS feed discovery to the cold start bootstrap, and a durable RSS scraper path so the scout keeps polling feeds for new articles on every run. This supplements the existing bootstrap (WebQuery seeds + platform sources) with real reporting from local outlets — giving day-one signal density and accelerating actor discovery.

## Problem Statement / Motivation

The current cold start generates search queries about problems that *might* exist. The scout has to run multiple cycles before it builds meaningful signal density. Local news RSS feeds flip this: they provide problems that are *actually being reported*, named actors (organizations, officials, nonprofits), and geographic specificity — all from the first run.

RSS feeds also do double duty: they seed the cold start *and* become persistent sources the scout polls on every subsequent run.

## Proposed Solution

### Phase 1: Bootstrap — Outlet Discovery & Feed Registration

During `Bootstrapper::run()`, after existing platform source generation:

1. **LLM outlet discovery** — Call Claude Haiku with the city name/region and ask for up to 8 local news outlets (newspapers, alt-weeklies, TV stations, hyperlocal blogs). Return outlet name + homepage URL as structured JSON. Prompt explicitly excludes national wire services (AP, Reuters, NPR national).

2. **RSS feed URL discovery** — For each outlet, two-step discovery:
   - **LLM guess**: Claude likely knows common feed URL patterns for major outlets
   - **Mechanical fallback**: Fetch homepage via `reqwest`, parse HTML for `<link rel="alternate">` tags matching `application/rss+xml` or `application/atom+xml`

3. **Store as SourceNodes** — Each discovered feed becomes a `SourceNode` with:
   - `source_type: SourceType::Rss`
   - `discovery_method: DiscoveryMethod::ColdStart`
   - `source_role: SourceRole::Mixed` (news covers both tensions and responses)
   - `url: Some(feed_url)` (the RSS/Atom endpoint, not the homepage)
   - `canonical_value`: outlet name (e.g., "Star Tribune")
   - `gap_context: Some(format!("Outlet: {name}"))`
   - `weight: 0.5` (standard cold start weight)

### Phase 2: Scout Pipeline — RSS Scraper Path

Add a new dispatch branch in `scrape_phase()` for `SourceType::Rss`:

1. **Fetch feed XML** via `reqwest` (not Chrome — RSS is plain XML)
2. **Parse with `feed-rs`** crate (handles RSS 2.0, Atom, JSON Feed)
3. **Extract article URLs** — take the 10 most recent items by `pub_date`, with a 30-day recency cutoff. Items without `pub_date` are included (up to the cap).
4. **Dedup against previously seen URLs** — check each article URL against the existing `content_already_processed` hash check (URL-based dedup already exists in the pipeline)
5. **Push new article URLs into `phase_urls`** — they flow through the standard Chrome/Browserless scrape → LLM extraction → dedup → Neo4j write pipeline

### Phase 3: Ongoing Lifecycle

RSS `SourceNode` participates in the standard source lifecycle:
- **Scheduling**: Weight-based cadence via `cadence_hours_for_weight()` (same as Web sources)
- **Quality evolution**: `signals_produced` and `consecutive_empty_runs` drive weight up/down
- **Deactivation**: Standard 10+ consecutive empty runs threshold applies
- No RSS-specific scheduling — the existing weight system handles feed frequency naturally

## Technical Considerations

### New Dependencies

- `feed-rs = "2"` — workspace dependency for RSS 2.0 / Atom / JSON Feed parsing
- `reqwest` — already a workspace dependency, used for lightweight feed fetching

### Files Changed

| File | Change |
|---|---|
| `modules/rootsignal-common/src/types.rs` | Add `Rss` variant to `SourceType` enum + all match arms (`Display`, `from_str_loose`, `from_url`, `is_query`, `link_pattern`) |
| `modules/rootsignal-scout/src/sources.rs` | Add `SourceType::Rss` case to `canonical_value_from_url()` |
| `modules/rootsignal-scout/src/bootstrap.rs` | Add `discover_news_outlets()` and `discover_rss_feeds()` methods |
| `modules/rootsignal-scout/src/scraper.rs` | Add `RssFetcher` struct with `fetch_items(url) -> Result<Vec<FeedItem>>` |
| `modules/rootsignal-scout/src/scout.rs` | Add `SourceType::Rss` branch in `scrape_phase()` dispatch (~line 1010) |
| `Cargo.toml` (root) | Add `feed-rs` workspace dependency |
| `modules/rootsignal-scout/Cargo.toml` | Reference `feed-rs` workspace dep |

### Architecture Decisions

- **Feed URL is the SourceNode URL**, not the outlet homepage. Mirrors how Reddit stores the subreddit URL.
- **`reqwest` for feed fetching**, not Chrome/Browserless. RSS is plain XML — Chrome is 10-100x slower and wastes container resources.
- **Article URLs are transient**, not stored as separate SourceNodes. They flow through `phase_urls` like WebQuery result URLs do. This avoids source node explosion (10 articles × 8 feeds × N runs = thousands of Web sources).
- **`SourceRole::Mixed`** for all RSS sources. News covers both tensions and responses. Phase A picks them up.
- **`feed-rs`** over `rss` crate — handles RSS 2.0, Atom, and JSON Feed in a single parser.
- **Atom feeds are in scope** — the discovery selector matches both `application/rss+xml` and `application/atom+xml`.

### Failure Handling

- **LLM outlet discovery fails**: Log warning, skip RSS sources entirely. Bootstrap continues with existing WebQuery + platform sources. Consistent with `discover_subreddits` pattern.
- **Outlet homepage doesn't resolve / 5xx**: Log warning, skip that outlet, continue with others.
- **No RSS feed found for an outlet**: Skip it. Not all outlets have feeds.
- **Feed returns malformed XML**: Log warning with feed URL, skip. `feed-rs` handles most edge cases.
- **Feed returns 0 items**: Store the SourceNode anyway — an empty feed today may have items tomorrow. Matches existing `signals_produced: 0` pattern.
- **All outlets fail**: Bootstrap still succeeds with WebQuery + platform sources. RSS is purely additive.

### Gotchas from Institutional Learnings

- **Never `unwrap_or` on LLM-extracted fields** — if `pub_date` parsing fails, use `Option<DateTime<Utc>>` with `None`, not a default. Let quality scorer handle it.
- **Source upsert is idempotent** (MERGE on url) — re-running bootstrap won't create duplicate RSS SourceNodes.
- **Multi-city isolation** — RSS sources must have `city` field set and be scoped to the correct bounding box.

## Acceptance Criteria

- [x] `SourceType::Rss` variant exists and round-trips through Neo4j correctly
- [x] Bootstrap for a new city discovers 3-8 local news outlets and registers their RSS feeds as SourceNodes
- [x] Scout `scrape_phase` fetches RSS feed XML, parses articles, and pushes URLs through the extraction pipeline
- [x] Articles from RSS feeds produce signals (Tension, Aid, Gathering, etc.) visible in the graph
- [x] RSS SourceNodes participate in weight-based scheduling and deactivation
- [x] Bootstrap completes successfully even when all RSS discovery fails (graceful degradation)
- [x] Re-running bootstrap does not create duplicate RSS SourceNodes

## Success Metrics

- Cold-started cities have 2-3x more signals after the first scout run compared to current bootstrap
- RSS-discovered signals contain named actors, accelerating source discovery via `SourceFinder`
- At least 50% of discovered outlets have parseable RSS/Atom feeds

## Dependencies & Risks

- **`feed-rs` crate maturity** — well-maintained, 2M+ downloads, handles format edge cases. Low risk.
- **LLM outlet quality** — Claude may hallucinate outlets for smaller cities. Mitigated by mechanical feed URL validation (if homepage doesn't resolve, outlet is skipped).
- **Paywall content** — local newspaper articles behind paywalls will produce thin extraction. This is acceptable — the bounding box filter and quality scorer handle it downstream. Not worth blocking on.

## References & Research

- Brainstorm: `docs/brainstorms/2026-02-20-news-rss-cold-start-brainstorm.md`
- Bootstrap implementation: `modules/rootsignal-scout/src/bootstrap.rs`
- SourceType enum: `modules/rootsignal-common/src/types.rs:456-546`
- Scout scrape dispatch: `modules/rootsignal-scout/src/scout.rs:1000-1236`
- Scraper infrastructure: `modules/rootsignal-scout/src/scraper.rs`
- Source scheduling: `modules/rootsignal-scout/src/scheduler.rs`
- Source writer: `modules/rootsignal-graph/src/writer.rs:1527-1577`
- Existing subreddit discovery pattern: `modules/rootsignal-scout/src/bootstrap.rs:229-263`
- Unwrap-or anti-pattern: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
