---
date: 2026-02-22
topic: archive-crawl
---

# Web Crawling in Archive

## What We're Building

A `crawl()` method on Archive that does BFS web crawling using archive's existing browserless-backed `page()` infrastructure. No separate crate — just a BFS discovery algorithm that calls `archive.page(url)` for each URL and follows links.

```rust
// Shorthand — sensible defaults
let pages = archive.crawl(url).await?;

// Builder via source handle — full control
let pages = archive.source(url).await?.crawl()
    .max_depth(2)
    .limit(20)
    .include("/about")
    .await?;
```

No AI pipeline. Just discover and fetch.

## Why This Approach

**Fold into archive (no separate crate):**
- Archive already owns page fetching via browserless/Chrome — crawl is just BFS over `archive.page()`
- The crawl logic is a traversal algorithm, not a standalone service
- No new crate, no new dependency wiring
- Pages are SPA-heavy so raw HTTP fetch is useless — must go through browserless anyway

**Archive already has all the pieces:**
- Browserless page rendering (handles SPAs)
- HTML→markdown conversion
- Link extraction from HTML (`extract_all_links`)
- Page storage with content hashing

**Crawl is just the glue:**
1. Fetch seed page via `archive.page(url)`
2. Extract links from `page.links` (already populated)
3. Filter by same-host + include/exclude patterns
4. BFS to depth N, up to limit M
5. Return all collected pages

## Key Decisions

- **No separate crate**: Crawl is a BFS algorithm over archive's existing page() + links. Not enough standalone value for a separate crate.
- **Browserless for all fetching**: Pages are fetched through archive's existing browserless/Chrome backend. No raw HTTP — SPAs need JS rendering.
- **API surface**: `archive.crawl(url)` shorthand + `source().crawl().max_depth(2).limit(20)` builder, consistent with existing archive API patterns.
- **Returns `Vec<ArchivedPage>`**: Pages go through the normal archive pipeline (fetch, convert, store). Caller gets back stored pages with markdown, links, title, etc.
- **Same-host by default**: BFS stays on the seed URL's domain. No cross-domain following.
- **Cross-service traversal stays in scout**: Scout orchestrates IG → website → Facebook because that routing is domain-specific business logic.

## Primary Use Case

Scout crawling a business website to extract contact info, social links, hours:

```rust
// Today: scout manually fetches homepage, extracts links, fetches those, etc.
// Tomorrow:
let pages = archive.crawl("https://localcoffeeshop.com").await?;
for page in &pages {
    // Extract contact info, social links, hours, menus from page.markdown
}
```

## Open Questions

- Rate limiting between page fetches: fixed delay, or respect browserless concurrency limits?
- Should crawl deduplicate against already-archived pages, or always re-fetch?

## Next Steps

→ `/workflows:plan` for implementation details
