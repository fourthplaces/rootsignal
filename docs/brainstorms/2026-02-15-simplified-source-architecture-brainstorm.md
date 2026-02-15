---
date: 2026-02-15
topic: simplified-source-architecture
---

# Simplified Source Architecture

## What We're Building

Flatten the source model so that a source is just a URL (or a search query). Drop `source_type`, drop `website_sources` and `social_sources` child tables, and derive everything — adapter, cadence, name, handle — from the URL at runtime.

## Why This Approach

The current architecture stores `source_type` as a column, but `parse_source_input()` already infers it from the URL domain. The child tables (`website_sources`, `social_sources`) duplicate fields that already exist on `sources` (`url`, `handle`) and only add uniqueness constraints. This is premature normalization that adds indirection without value.

The URL contains all the information needed:
- `instagram.com/somecoffeeshop` → Instagram profile, apify_instagram adapter
- `instagram.com/explore/search/keyword/?q=ICE%20minneapolis` → Instagram search, apify_instagram_search adapter
- `facebook.com/somepage` → Facebook page, apify_facebook adapter
- `example.com/news` → Generic website, spider adapter
- No URL (plain text) → Web search, web_searcher

## Key Decisions

- **Drop `source_type` column**: Derive adapter and scheduling category from URL domain at runtime.
- **Drop `website_sources` table**: `domain` and `max_crawl_depth` are no longer stored. Domain is derived from URL. Crawl depth is a system-level concern, not per-source config.
- **Drop `social_sources` table**: `platform` and `handle` are derived from URL. Handle is still stored on `sources` as a convenience field.
- **Normalize URLs on input**: A `normalize_and_classify` function replaces `parse_source_input`. It canonicalizes URLs before storage — strip `www.`, trailing slashes, query params (except meaningful ones like `?q=`), alias domains (`fb.com` → `facebook.com`, `twitter.com` → `x.com`), and force `https://`.
- **Dedup via `UNIQUE(url)`**: Normalized URLs make `UNIQUE(url)` sufficient. No need for child table uniqueness constraints.
- **Always store `https://`**: Protocol is always HTTPS. Normalize all inputs to `https://`.
- **Keep `max_crawl_depth` as system default**: Remove per-source crawl depth config. System decides depth.

## Simplified `sources` Table

Columns kept:
- `id`, `entity_id`, `name`
- `url` (nullable — null means web search)
- `handle` (convenience, derived from URL)
- `is_active`, `last_scraped_at`, `consecutive_misses`
- `qualification_status`, `qualification_summary`, `qualification_score`, `content_summary`
- `config` (JSONB — search queries, include/exclude patterns)
- `created_at`

Columns dropped:
- `source_type`
- `adapter`
- `cadence_hours`

Computed at runtime:
- Adapter selection: domain → adapter mapping
- Cadence: domain → category (social/search/website) → base hours
- Source category: derived from domain for scheduling

## URL Normalization Rules

- **Social profiles**: strip `www.`, trailing slashes, query params. Canonicalize aliases. Result: `https://{domain}/{handle}`
- **Instagram search**: preserve `?q=` param, strip everything else
- **Generic websites**: strip `www.`, strip tracking params (`utm_*`, `ref`, `fbclid`), keep meaningful path/query
- **All**: force `https://`, lowercase domain

## Open Questions

- None — ready for planning.

## Next Steps

→ `/workflows:plan` for implementation details
