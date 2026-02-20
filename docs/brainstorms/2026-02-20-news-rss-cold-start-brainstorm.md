---
date: 2026-02-20
topic: news-rss-cold-start
---

# News RSS Feeds for Cold Start Enrichment

## What We're Building

Supplement the existing cold start bootstrap with local news RSS feeds. During bootstrap, Claude identifies local news outlets for the city (newspapers, alt-weeklies, TV stations, hyperlocal blogs), discovers their RSS feed URLs, and ingests them as both immediate signal and ongoing sources.

The current bootstrap generates search queries about problems that *might* exist. News feeds give us problems that are *actually being reported* — plus they're actor-rich, which accelerates source discovery for subsequent scout runs.

## Why This Approach

- **Serper/Google News** was considered but adds another aggregation layer; local outlets are higher signal-to-noise
- **Full article scraping at bootstrap time** was considered but RSS is lighter weight and structured
- **RSS does double duty** — cold start signal *and* a persistent source the scout keeps polling
- **Claude already knows local outlets** — the same pattern used for generating subreddit names during bootstrap

## How It Fits in the Pipeline

1. Bootstrap runs (city doesn't exist yet)
2. **New step:** Claude generates a list of local news outlets for the city
3. For each outlet, discover RSS feed URLs — Claude likely knows common patterns, plus scrape the homepage for `<link rel="alternate" type="application/rss+xml">` tags as fallback
4. Parse the RSS feed, extract recent article URLs
5. Store each feed as a new `Source` node (new `SourceType::Rss`) with `discovery_method: ColdStart`
6. Article URLs become scrape targets for the first scout run

## Key Decisions

- **New `SourceType::Rss`** variant needed in `rootsignal-common`
- **RSS parsing** via `feed-rs` crate (handles RSS 2.0, Atom, JSON Feed)
- **Feed discovery** is LLM-assisted (Claude names outlets + feed URLs) with mechanical fallback (scrape homepage for RSS link tags)
- **Feeds persist as sources** so the scout keeps polling them on subsequent runs

## Open Questions

- Does the scout need an RSS-specific scraper path (parse feed -> extract article URLs -> scrape articles), or treat each article URL as a regular `Web` source?
- How many outlets per city? 5-10 feels right but needs validation
- Should RSS feeds get their own scheduling cadence (e.g. check every 6 hours vs daily)?

## Next Steps

-> `/workflows:plan` for implementation details
