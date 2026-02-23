---
title: "feat: Admin Archive Dashboard"
type: feat
date: 2026-02-22
---

# Admin Archive Dashboard

## Overview

Add a new `/archive` page to the admin app that surfaces scraped content from the PostgreSQL archive tables. Provides operational monitoring (is scraping healthy?), volume analytics (ingestion trends by content type), and light content exploration (browse recent items per type).

## Problem Statement

The admin app currently focuses on **extracted signals** (the intelligence layer) but provides no visibility into the **raw scraped content** sitting in the archive tables (posts, short_videos, stories, long_videos, pages, feeds, search_results, files). Admins cannot see what content was actually collected, how much, or from which sources without querying the database directly.

## Proposed Solution

A new top-level page at `/archive` following existing admin patterns:

1. **Summary stat cards** -- total row counts for all 8 content types
2. **7-day ingestion volume chart** -- stacked area chart (Recharts) showing daily scrape counts by type
3. **Tabbed content tables** -- one tab per content type with recent items (limit 50)

## Technical Approach

### Architecture

```
┌─────────────────────────────────────────────────────┐
│  ArchivePage.tsx                                     │
│  ┌──────────┐ ┌──────────────────────────────────┐  │
│  │ Stat     │ │ Ingestion Volume Chart            │  │
│  │ Cards    │ │ (Recharts AreaChart, 7 days)      │  │
│  │ (8 cards)│ │                                    │  │
│  └──────────┘ └──────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐│
│  │ Tabs: Posts | Reels | Stories | Videos | Pages  ││
│  │        | Feeds | Search Results | Files         ││
│  │ ┌──────────────────────────────────────────────┐││
│  │ │ Table (recent 50 items, type-specific cols)  │││
│  │ └──────────────────────────────────────────────┘││
│  └──────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────┘
```

### GraphQL Queries (3 separate queries, ScoutPage pattern)

**Query 1: `adminArchiveCounts`** -- fires on page load
```graphql
query AdminArchiveCounts {
  adminArchiveCounts {
    posts
    shortVideos
    stories
    longVideos
    pages
    feeds
    searchResults
    files
  }
}
```

**Query 2: `adminArchiveVolume(days: Int)`** -- fires on page load
```graphql
query AdminArchiveVolume($days: Int) {
  adminArchiveVolume(days: $days) {
    day
    posts
    shortVideos
    stories
    longVideos
    pages
    feeds
    searchResults
    files
  }
}
```

**Query 3: One per tab** -- fires with `skip` when tab is not active
```graphql
query AdminArchivePosts($limit: Int) {
  adminArchivePosts(limit: $limit) {
    id
    sourceUrl      # JOIN sources.url
    permalink
    author
    textPreview    # server-truncated to 150 chars
    hashtags
    engagementSummary  # server-formatted string: "100 likes, 5 comments"
    publishedAt
  }
}
```

Similar queries for each content type with type-specific fields.

### Design Decisions

**Engagement display**: Server-side formatting into a human-readable string (e.g., "1.2k likes, 45 comments"). The resolver iterates JSONB keys, formats numbers with abbreviations, and joins them. This keeps the frontend simple and handles heterogeneous platform schemas in one place.

**"Source URL" vs permalink**: Show `permalink` as the clickable link (the actual content URL). Derive platform name from `sources.url` domain (e.g., "instagram.com" -> "Instagram"). Fall back to `sources.url` when permalink is null.

**Text preview**: Server-truncated to 150 characters with `...` suffix. No client-side expand.

**Default tab**: "Posts" (first tab, most common content type).

**Row limit**: 50 per tab, hardcoded. No user-configurable limit.

**Polling**: None. Consistent with DashboardPage -- data refreshes on page load only.

**Auth**: Admin-only via `AdminGuard` on all resolvers.

**Tab naming**: Use "Reels" instead of "Short Videos" (user-facing clarity). Use "Stories" for ephemeral content (context of the archive page disambiguates from signal stories). Use "Videos" instead of "Long Videos".

**Files tab columns**: `url, title, mime_type, duration (if present), page_count (if present), fetched_at`.

### Implementation Phases

#### Phase 1: Backend -- Database queries + GraphQL resolvers

**New file: `modules/rootsignal-api/src/db/models/archive.rs`**

Follow the `scout_run.rs` pattern (tuple queries with `sqlx::query_as`, manual row conversion).

Functions needed:
- `count_all(pool) -> Result<ArchiveCounts>` -- 8 COUNT queries via `tokio::join!` for parallelism
- `volume_by_day(pool, days) -> Result<Vec<ArchiveVolumeDay>>` -- UNION ALL across 8 tables grouped by `date_trunc('day', fetched_at)`
- `recent_posts(pool, limit) -> Result<Vec<ArchivePost>>` -- JOIN sources, truncate text, format engagement
- `recent_short_videos(pool, limit) -> Result<Vec<ArchiveShortVideo>>`
- `recent_stories(pool, limit) -> Result<Vec<ArchiveStory>>`
- `recent_long_videos(pool, limit) -> Result<Vec<ArchiveLongVideo>>`
- `recent_pages(pool, limit) -> Result<Vec<ArchivePage>>`
- `recent_feeds(pool, limit) -> Result<Vec<ArchiveFeed>>` -- `jsonb_array_length(items)` for item count
- `recent_search_results(pool, limit) -> Result<Vec<ArchiveSearchResult>>` -- `jsonb_array_length(results)` for result count
- `recent_files(pool, limit) -> Result<Vec<ArchiveFile>>`

Register in:
- `modules/rootsignal-api/src/db/models/mod.rs` -- `pub mod archive;`
- `modules/rootsignal-api/src/db/mod.rs` -- `pub use models::archive;`

**Updates to: `modules/rootsignal-api/src/graphql/schema.rs`**

New `#[derive(SimpleObject)]` types:
- `ArchiveCounts` -- 8 `i64` fields
- `ArchiveVolumeDay` -- `day: String` + 8 `i64` fields
- `ArchivePost`, `ArchiveShortVideo`, `ArchiveStory`, `ArchiveLongVideo`, `ArchivePage`, `ArchiveFeed`, `ArchiveSearchResult`, `ArchiveFile`

New resolvers (all with `#[graphql(guard = "AdminGuard")]`):
- `admin_archive_counts() -> Result<ArchiveCounts>`
- `admin_archive_volume(days: Option<u32>) -> Result<Vec<ArchiveVolumeDay>>`
- `admin_archive_posts(limit: Option<u32>) -> Result<Vec<ArchivePost>>`
- ... (one per content type)

Helper function:
- `format_engagement(engagement: Option<serde_json::Value>) -> String` -- iterates JSONB keys, formats values with k/M abbreviations, joins with commas

#### Phase 2: Frontend -- Page, routing, navigation

**New file: `modules/admin-app/src/pages/ArchivePage.tsx`**

Pattern: Follow `ScoutPage.tsx` for tabs, `DashboardPage.tsx` for stat cards and chart.

```
Tab type: "posts" | "reels" | "stories" | "videos" | "pages" | "feeds" | "search" | "files"

State:
- tab: useState<Tab>("posts")

Queries:
- ADMIN_ARCHIVE_COUNTS -- always fires
- ADMIN_ARCHIVE_VOLUME -- always fires (days: 7)
- ADMIN_ARCHIVE_POSTS -- fires when tab === "posts"
- ADMIN_ARCHIVE_SHORT_VIDEOS -- fires when tab === "reels"
- ... etc, all with skip: tab !== "x"

Layout:
1. <h1>Archive</h1>
2. Stat cards grid (4 cols on md, 8 cols on lg -- or 2 rows of 4)
3. Stacked area chart (ResponsiveContainer + AreaChart)
4. Tab bar
5. Content table (columns vary by tab)
```

**Updates to: `modules/admin-app/src/graphql/queries.ts`**

Add 10 new queries:
- `ADMIN_ARCHIVE_COUNTS`
- `ADMIN_ARCHIVE_VOLUME`
- `ADMIN_ARCHIVE_POSTS`
- `ADMIN_ARCHIVE_SHORT_VIDEOS`
- `ADMIN_ARCHIVE_STORIES`
- `ADMIN_ARCHIVE_LONG_VIDEOS`
- `ADMIN_ARCHIVE_PAGES`
- `ADMIN_ARCHIVE_FEEDS`
- `ADMIN_ARCHIVE_SEARCH_RESULTS`
- `ADMIN_ARCHIVE_FILES`

**Updates to: `modules/admin-app/src/layouts/AdminLayout.tsx`**

Add to `navItems` array:
```tsx
{ to: "/archive", label: "Archive" }
```

Position: After "Scout", before "Situations" (archive is closely related to scouting).

**Updates to: `modules/admin-app/src/App.tsx`**

Add route:
```tsx
<Route path="archive" element={<ArchivePage />} />
```

Add import for `ArchivePage`.

## Tab Column Definitions

| Tab | Columns |
|-----|---------|
| Posts | Permalink, Author, Text Preview, Platform, Hashtags, Engagement, Published |
| Reels | Permalink, Text Preview, Engagement, Published |
| Stories | Permalink, Text Preview, Location, Expires, Fetched |
| Videos | Permalink, Text Preview, Engagement, Published |
| Pages | URL, Title, Fetched |
| Feeds | URL, Title, Items, Fetched |
| Search Results | Query, Results, Fetched |
| Files | URL, Title, Type, Duration, Pages, Fetched |

## Acceptance Criteria

### Functional

- [x] `/archive` route renders the ArchivePage behind AdminGuard
- [x] "Archive" appears in sidebar navigation between Scout and Situations
- [x] 8 stat cards show total row counts for each content type
- [x] Stacked area chart shows 7-day ingestion volume broken down by content type
- [x] 8 tabs render with type-specific table columns
- [x] Tab switching loads data for the active tab only (skip pattern)
- [x] Engagement JSONB renders as formatted string (e.g., "1.2k likes, 5 comments")
- [x] Text previews are truncated to 150 characters
- [x] Permalinks open in new tab
- [x] Platform name derived from source URL domain

### Edge Cases

- [x] Empty archive (0 rows in all tables) shows 0 counts and empty chart gracefully
- [x] Null engagement renders as empty string or "-"
- [x] Null permalink falls back to source URL
- [x] Expired stories render with muted text styling
- [x] Chart shows 0 for days with no scraping activity

### Non-Functional

- [x] Summary counts query completes in <1s (parallel COUNT queries)
- [x] Tab data loads in <500ms for limit=50

## Files Changed

### New Files
- `modules/rootsignal-api/src/db/models/archive.rs` -- DB query functions
- `modules/admin-app/src/pages/ArchivePage.tsx` -- React page component

### Modified Files
- `modules/rootsignal-api/src/db/models/mod.rs` -- register archive module
- `modules/rootsignal-api/src/db/mod.rs` -- re-export archive module
- `modules/rootsignal-api/src/graphql/schema.rs` -- GraphQL types + resolvers
- `modules/admin-app/src/graphql/queries.ts` -- 10 new queries
- `modules/admin-app/src/layouts/AdminLayout.tsx` -- nav item
- `modules/admin-app/src/App.tsx` -- route + import

## Dependencies & Risks

- **Performance**: 8 parallel COUNT queries on unpartitioned tables. Fine for now (<100k rows per table). If tables grow large, consider materialized views or `pg_stat_user_tables.n_live_tup` approximation.
- **Pre-existing issue**: The `pages` table may be missing a `raw_html` column that `store.rs` references. This is not caused by this feature but could surface during testing.
- **No region scoping**: Archive tables have no region column. Unlike other admin pages that filter by "twincities", the archive page shows all data globally. This is correct behavior since sources aren't region-partitioned in Postgres.

## References

### Internal
- `modules/admin-app/src/pages/DashboardPage.tsx` -- stat cards + chart pattern
- `modules/admin-app/src/pages/ScoutPage.tsx` -- tab pattern with `skip`
- `modules/rootsignal-api/src/db/models/scout_run.rs` -- DB query pattern
- `modules/rootsignal-api/src/graphql/schema.rs:673` -- PgPool unwrap pattern
- `modules/rootsignal-api/migrations/004_content_type_tables.sql` -- table schemas
- `modules/rootsignal-api/migrations/005_post_metadata.sql` -- post metadata columns
