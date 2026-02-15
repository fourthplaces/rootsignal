---
date: 2026-02-15
topic: page-snapshot-link-graph
---

# Page Snapshot Link Graph

## What We're Building

A cross-source link graph that tracks outbound links from every page snapshot, with enough surrounding context for an LLM to decide whether to follow them. Links connect snapshots across sources, enabling authority analysis, source discovery, signal corroboration, and intelligent LLM-driven crawling.

## Why This Approach

We considered capturing links as a separate activity or during scraping, but storing them at `store_page_snapshot` time is simplest — the HTML is already available, and links are part of the snapshot's immutable record. No extra pipeline step needed.

For the schema, we chose to store rich context (anchor text, surrounding text, section) rather than bare edges because the primary consumer is an LLM deciding whether to follow a link. The richer the context, the better the decision.

## Key Decisions

- **Store at snapshot time**: Link extraction happens inside `store_page_snapshot`, reusing existing HTML parsing logic from the HTTP adapter.
- **Nullable target_snapshot_id**: The target URL may not have been scraped yet. Populated lazily when/if we scrape it, allowing the graph to connect across sources without requiring the target to exist.
- **Rich context per link**: `anchor_text`, `surrounding_text` (~200 char window), and `section` (nav/header/body/footer/sidebar) give the LLM enough to assess relevance.
- **Deduped per snapshot**: `UNIQUE(source_snapshot_id, target_url)` prevents duplicate edges from the same page.

## Schema

```sql
CREATE TABLE page_snapshot_links (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_snapshot_id UUID NOT NULL REFERENCES page_snapshots(id) ON DELETE CASCADE,
    target_url TEXT NOT NULL,
    target_snapshot_id UUID REFERENCES page_snapshots(id) ON DELETE SET NULL,
    anchor_text TEXT,
    surrounding_text TEXT,
    section TEXT,
    UNIQUE(source_snapshot_id, target_url)
);

CREATE INDEX idx_page_snapshot_links_source ON page_snapshot_links(source_snapshot_id);
CREATE INDEX idx_page_snapshot_links_target_url ON page_snapshot_links(target_url);
CREATE INDEX idx_page_snapshot_links_target_snapshot ON page_snapshot_links(target_snapshot_id) WHERE target_snapshot_id IS NOT NULL;
```

## What It Enables

- **Entity authority**: Query which entities are most linked-to across sources
- **Source discovery**: Find `target_url`s that don't match any existing source
- **Signal corroboration**: Count independent sources linking to the same target
- **LLM crawl decisions**: Provide `anchor_text` + `surrounding_text` + `section` as context for "should I follow this link?"

## Open Questions

- Should `surrounding_text` be extracted at store time or lazily on first LLM access?
- Should we filter out obviously low-value links (privacy policy, terms of service, social media share buttons) at extraction time or let the LLM decide?
- Should `target_snapshot_id` be backfilled on a schedule, or only populated when a link is explicitly followed?

## Next Steps

→ `/workflows:plan` for implementation details
