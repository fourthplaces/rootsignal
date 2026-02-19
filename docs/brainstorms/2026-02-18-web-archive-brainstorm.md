---
date: 2026-02-18
topic: web-archive
---

# Web Archive

## What We're Building

A standalone Rust crate (`web-archive`) that sits between scout and the outside world. Every web interaction — page scrapes, search queries, social media fetches — flows through this layer. It delegates to the real implementations (Chrome, Serper, Apify) and records every request and response to Postgres. The crate owns its own connection pool; scout doesn't know Postgres exists.

This gives us a historical record of everything scout has ever seen, enabling extraction replay, regression testing on real data, and discovery analysis — all without hitting the network.

## Why This Approach

Today, raw scraped content is ephemeral. Chrome renders a page, Readability extracts markdown, the extractor pulls signals, and the original content is gone. The graph stores signals and source metadata, but not what produced them. This means:

- We can't re-extract old pages when we improve prompts
- We can't regression-test the extractor against real content
- We can't analyze which search queries actually yield high-value pages
- We can't replay extraction deterministically
- We pay for the same scrape again if we need the content later

SimWeb solves the testing problem with synthetic content, but synthetic pages don't surface the messy edge cases real pages do (broken HTML, paywalls, redirect chains, content that looks like a signal but isn't).

A cache-of-record (always write, read-on-demand) avoids staleness problems. Production always hits the real web and records. Replay mode only reads, never hits the network.

## Prerequisites

Before building the crate, two things need to happen in the existing codebase:

**Extract scraper traits to a shared crate.** `PageScraper`, `WebSearcher`, `SocialScraper` and their associated types (`SearchResult`, `SocialPost`, `SocialAccount`, `SocialPlatform`) currently live in `rootsignal-scout::scraper`. The archive crate needs to implement these traits, and scout's binary needs to construct an `Archive` — creating a circular dependency. Move the traits and types into `rootsignal-common` (or a new `rootsignal-scraper-traits` crate) so both `rootsignal-scout` and `web-archive` can depend on them without a cycle.

**Add serde derives to scraper types.** `SearchResult`, `SocialPost`, `SocialAccount`, `SocialPlatform` only derive `Debug + Clone`. The archive needs `Serialize + Deserialize` to round-trip them through `response_body`.

**Extract `content_hash` to `rootsignal-common`.** The FNV-1a hash function is duplicated between `scout.rs` and `investigator.rs`, and the two copies hash different things (page content vs URL). Consolidate into one shared function before adding a third consumer.

## Core Design

### Schema

```sql
CREATE TABLE runs (
    run_id            UUID PRIMARY KEY,
    city_slug         TEXT NOT NULL,
    started_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at       TIMESTAMPTZ,
    status            TEXT NOT NULL DEFAULT 'running',  -- 'running', 'completed', 'failed'
    interaction_count INTEGER DEFAULT 0
);

CREATE TABLE web_interactions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id        UUID NOT NULL REFERENCES runs(run_id),
    city_slug     TEXT NOT NULL,
    kind          TEXT NOT NULL,       -- 'scrape', 'scrape_raw', 'search', 'social_posts', 'social_hashtags'
    key           TEXT NOT NULL,       -- url for scrapes, query for searches, account id for social
    sequence      INTEGER NOT NULL DEFAULT 0,  -- monotonic per (run_id, kind, key) for replay ordering
    params_json   JSONB,              -- max_results, hashtags (sorted), limit, etc.
    response_body TEXT,               -- markdown for scrapes, serialized JSON for searches/social. NULL on error.
    content_hash  TEXT,               -- FNV-1a of response_body, same algo as rootsignal-common
    fetched_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_ms   INTEGER,
    error         TEXT                -- null on success, error message on failure
) PARTITION BY RANGE (fetched_at);

-- Monthly partitions (create new ones ahead of time)
CREATE TABLE web_interactions_2026_02 PARTITION OF web_interactions
    FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');

-- Replay: load all interactions for a run in order
CREATE INDEX idx_web_interactions_replay ON web_interactions (run_id, kind, key, sequence);

-- Lookup: most recent scrape of a URL
CREATE INDEX idx_web_interactions_key_fetched ON web_interactions (key, fetched_at DESC);

-- Discovery analysis: queries by kind in time windows
CREATE INDEX idx_web_interactions_kind_fetched ON web_interactions (kind, fetched_at DESC);

-- Filter by city
CREATE INDEX idx_web_interactions_city ON web_interactions (city_slug, fetched_at DESC);
```

One table (partitioned), not three. `kind` + `key` discriminates. `runs` tracks completeness so replay can distinguish finished runs from crashed ones.

### Crate structure

```
modules/web-archive/
├── Cargo.toml          -- sqlx, serde, async-trait, uuid, chrono
├── migrations/
│   ├── 001_create_runs.sql
│   └── 002_create_web_interactions.sql
└── src/
    ├── lib.rs          -- pool init, pub mod archive/replay/types
    ├── types.rs        -- WebInteraction struct, Kind enum
    ├── archive.rs      -- Archive: wraps real impls, records to Postgres
    └── replay.rs       -- Replay: reads from Postgres only, no network
```

### Key types

**Archive** wraps real `PageScraper` + `WebSearcher` + `SocialScraper` implementations, delegates to them, records every interaction to Postgres, and itself implements all three traits. Scout talks to it like any other scraper. Must explicitly override both `scrape()` and `scrape_raw()` — the default impl of `scrape_raw` delegates to `scrape()`, which would silently record wrong content under the wrong kind.

**Replay** implements the same three traits but only reads from Postgres for a specific `run_id`. No network access. Errors loudly on cache miss so test failures are obvious rather than silently falling through. Only replays runs with `status = 'completed'` by default. Serves interactions ordered by `sequence` per `(kind, key)` tuple to handle cases where the same URL is scraped multiple times in a single run.

**ArchiveConfig** carries the `database_url`, `run_id`, and `city_slug`. Scout passes this at startup. The crate owns the `PgPool` (max 8 connections, min 1, acquire timeout 5s).

### How scout wires it up

Scout's `main.rs` currently constructs `ChromeScraper`, `SerperSearcher`, etc. directly. With the archive:

- **Production with archive**: `Archive::new(config, chrome, serper, social)` — real web, everything recorded
- **Production without archive**: If `WEB_ARCHIVE_DATABASE_URL` is not set, skip the archive layer entirely and use scrapers directly. Log a warning at startup. Matches the existing `NoopSocialScraper` pattern.
- **Test replay**: `Replay::new(config)` — no network, reads from a pinned run
- **SimWeb stays unchanged** — still valuable for synthetic/controlled scenarios

Scout passes `&dyn PageScraper`, `&dyn WebSearcher`, `&dyn SocialScraper` as it does today. The archive is invisible to everything downstream.

### Error handling contract

The archive must never kill production scraping. If the real scrape succeeds but the Postgres INSERT fails:
1. Log at `warn` level with the URL and error
2. Increment an error counter (for monitoring)
3. Return the real scraper's result to the caller

Postgres is a secondary concern. A PG outage means you lose the recording, not the scraping.

## What This Unlocks

**Extraction replay.** Change extractor prompts, feed archived content directly to `Extractor::extract()`, compare signal output. No Chrome, no Serper, no graph state needed. This is the highest-value use case.

**Regression testing on real data.** Pin a run_id as a test fixture. Assert that extractor changes don't silently drop signals from pages that used to produce them.

**Discovery analysis.** Query the archive: which search queries returned < 3 results? Which URLs produced zero signals? Filter by city. Feed this back into discovery tuning.

**Diff two extraction strategies.** Same archived content, two different prompt versions, compare outputs side by side.

**Budget efficiency.** Never pay twice for the same content when iterating on extraction logic.

## Scope of Replay

Replay has two distinct levels and it's important not to conflate them:

**Extraction replay (web layer only).** Feed archived content to the extractor. No graph needed. Deterministic. This is what the `Replay` struct provides and it's where most of the value is — prompt iteration, regression testing, extraction comparison.

**Full run replay (web + graph).** Re-execute the entire scout pipeline against historical web data. This requires not just archived web content but a snapshot of the graph state at the start of the run (sources, existing signals, dedup state). The archive alone is insufficient — dedup, scheduling, and clustering all read from Neo4j. Without matching graph state, a replayed run produces different results. This is a future capability that would require graph snapshots, not something the archive crate solves alone.

## Things to Watch

**Staleness masking.** The archive must not silently serve cached content in production. Production always hits the real web and records. Cache reads are opt-in (replay mode, explicit lookups). The content_hash dedup that already exists in the graph handles the "don't re-extract identical content" case.

**Storage growth.** Rough projections at current scale:

| Scale | Monthly | Yearly |
|---|---|---|
| 1 city, every 4 hours | 90-900 MB | 1-11 GB |
| 5 cities, every 4 hours | 450-4,500 MB | 5-54 GB |
| 20 cities, every 4 hours | 1.8-18 GB | 22-216 GB |

Postgres TOAST compresses TEXT values over 2KB automatically (typically 30-50% of original). Monthly range partitioning makes retention trivial — `DROP TABLE web_interactions_2025_06` is instant. Consider excluding or aggressively aging out `scrape_raw` interactions (full HTML, 50-200KB each) since they have low replay value — raw HTML is used for link extraction, not signal extraction.

**Error recording.** Failed fetches should be archived too (with `response_body = NULL` and `error` populated). Knowing that a URL was unreachable on a given date is valuable signal — it tells you about link rot, paywalls, rate limiting.

**Hashtag key ordering.** For `social_hashtags` interactions, sort hashtags alphabetically before joining into the `key` field. Otherwise the same set of hashtags in different order produces different keys, breaking replay lookups.

**Performance.** The Postgres INSERT adds ~1-2ms per interaction. Chrome scrapes take 1-30 seconds. The archive is less than 0.1% overhead on the critical path. No async buffering needed at current concurrency. Synchronous writes are fine.

**Postgres vs. extending Neo4j.** The archived content is tabular, not graphy. URLs, timestamps, blobs. Postgres is the right tool. Neo4j stays focused on the signal graph where relationship traversal matters.
