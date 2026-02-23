---
title: "feat: Add crawl() method to archive"
type: feat
date: 2026-02-22
---

# feat: Add crawl() method to archive

## Overview

Add BFS web crawling to archive using the existing browserless-backed `page()` infrastructure. The crawl method discovers pages by following links from a seed URL, respecting depth and limit constraints. No separate crate — this is a BFS algorithm over `archive.page()` + `page.links`.

## API

```rust
// Shorthand — sensible defaults (max_depth=2, limit=20)
let pages = archive.crawl("https://localcoffeeshop.com").await?;

// Builder via source handle — full control
let pages = archive.source("https://localcoffeeshop.com").await?.crawl()
    .max_depth(2)
    .limit(20)
    .include("/about")
    .exclude("/login")
    .await?;
```

Returns `Vec<ArchivedPage>` in BFS discovery order (seed first).

## Algorithm

1. Fetch seed page via internal page fetch (browserless/Chrome)
2. Use `page.links` (already extracted by `extract_all_links`) for link discovery
3. Filter links: same-host only + include/exclude substring patterns on path
4. BFS queue with depth tracking, up to `max_depth` and `limit`
5. Skip-and-continue on per-page fetch failures (seed failure = hard error)
6. Sequential fetching (one page at a time)
7. URL dedup via normalized visited set (strip fragments, trailing slashes)

## Implementation

### Phase 1: CrawlRequest builder + BFS core

#### `modules/rootsignal-archive/src/source_handle.rs`

- [x] Add `CrawlRequest` struct following existing builder pattern:
  ```rust
  pub struct CrawlRequest {
      inner: Arc<ArchiveInner>,
      source: Source,
      max_depth: usize,    // default: 2
      limit: usize,        // default: 20
      include_patterns: Vec<String>,
      exclude_patterns: Vec<String>,
  }
  ```
- [x] Add builder methods: `.max_depth(n)`, `.limit(n)`, `.include(pattern)`, `.exclude(pattern)`
- [x] Implement `IntoFuture` with standard boilerplate delegating to `send()`
- [x] Add `pub fn crawl(&self) -> CrawlRequest` to `SourceHandle`

#### `modules/rootsignal-archive/src/source_handle.rs` — `CrawlRequest::send()`

- [x] Implement BFS algorithm:
  - Use `VecDeque<(String, usize)>` as the queue (url, depth)
  - Use `HashSet<String>` as visited set with normalized URLs
  - Seed URL always fetched (not subject to include/exclude)
  - For each page: fetch via page service (reuse `PageRequest` flow or call services directly), extract links from result
  - Filter links: `same_host(link, seed)` + include/exclude on URL path
  - Enqueue filtered links at `depth + 1`
  - Stop when queue empty, limit reached, or depth exceeded
- [x] Add `normalize_crawl_url(url: &str) -> String` helper: strip fragment, strip trailing slash, lowercase host
- [x] Add `same_host(url: &str, seed: &str) -> bool` helper: compare `host_str()` values
- [x] Add `matches_patterns(url, include, exclude) -> bool` helper: include = OR'd substring on path, exclude = any match rejects. Include/exclude don't apply to seed.
- [x] Error handling: seed fetch failure returns `Err`. Child fetch failures logged + skipped.

### Phase 2: Shorthand + exports

#### `modules/rootsignal-archive/src/archive.rs`

- [x] Add shorthand method:
  ```rust
  pub async fn crawl(&self, url: &str) -> Result<Vec<ArchivedPage>> {
      self.source(url).await?.crawl().await
  }
  ```

#### `modules/rootsignal-archive/src/lib.rs`

- [x] Add `CrawlRequest` to public exports

### Phase 3: Tests

#### `modules/rootsignal-archive/src/source_handle.rs`

- [x] Unit test `normalize_crawl_url`: fragments stripped, trailing slashes stripped, idempotent
- [x] Unit test `same_host`: same host passes, subdomain rejects, different domain rejects
- [x] Unit test `matches_patterns`: include OR logic, exclude rejects, empty patterns = allow all, seed bypass

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Crate boundary | Inside archive | Crawl is BFS over archive.page() — not enough standalone value for a separate crate |
| Page fetching | Browserless/Chrome via existing services | SPAs need JS rendering; archive already has this |
| Concurrency | Sequential | Simple, safe, respects shared browserless instance |
| Error policy | Seed fail = error, child fail = skip | Crawl should be resilient; partial results better than no results |
| Same-host | Strict host_str() match | Subdomains excluded by default; use include patterns to add them |
| Include/exclude | Substring match on URL path, OR'd | Consistent with existing `extract_links_by_pattern` |
| URL dedup | Strip fragment + trailing slash | Prevents duplicate fetches for anchored/trailed URLs |
| Query strings | Keep (treat as distinct pages) | Paginated pages may have distinct content |
| Defaults | max_depth=2, limit=20 | Reasonable for business site crawls |
| max_depth(0) | Seed only | Useful for "just fetch this page" semantics |
| Return type | Vec\<ArchivedPage\> | Pages go through normal archive pipeline; caller gets stored pages |
| Return order | BFS order, seed first | Natural and predictable |
| DB strategy | Insert (no upsert) | Matches existing archive behavior; dedup is caller's concern |

## Acceptance Criteria

- [x] `archive.crawl(url).await?` fetches seed + follows links to depth 2, limit 20
- [x] `source().crawl().max_depth(1).limit(5).await?` respects builder params
- [x] `.include("/about")` only follows links containing "/about" in path
- [x] `.exclude("/login")` skips links containing "/login" in path
- [x] Same-host filtering: only follows links on the seed's domain
- [x] Seed page always returned first in results
- [x] Single failing child page doesn't abort the crawl
- [x] Seed page failure returns an error (not empty vec)
- [x] No duplicate URLs fetched (visited set works)
- [x] Fragment URLs deduplicated (e.g., /page#a and /page#b = one fetch)
- [x] `cargo check` passes, existing tests still pass
- [x] Unit tests for URL normalization, host matching, pattern filtering

## References

- Brainstorm: `docs/brainstorms/2026-02-22-crawl-ingestor-crate-brainstorm.md`
- Existing builder pattern: `source_handle.rs` — `PageRequest`, `SearchRequest`, etc.
- Link extraction: `links.rs` — `extract_all_links()`
- Page services: `services/page.rs` — `BrowserlessPageService`, `ChromePageService`
- Inspiration: `~/Developer/fourthplaces/mntogether/packages/extraction` — Ingestor trait, HttpIngestor BFS
