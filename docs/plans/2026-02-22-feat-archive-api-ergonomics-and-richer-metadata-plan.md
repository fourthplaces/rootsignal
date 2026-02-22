---
title: "feat: Archive API ergonomics and richer metadata extraction"
type: feat
date: 2026-02-22
---

# Archive API Ergonomics and Richer Metadata Extraction

## Overview

Enhance the Archive API with shorthand methods that skip the two-step `source().await?.method().await` pattern, extract richer metadata from posts (mentions, hashtags, media type, platform ID), and surface links from archived pages — all data that's either already available from platform APIs or cheaply extractable.

## Problem Statement

1. **Two-step boilerplate**: Every caller writes `archive.source(url).await?.search(query).await`. For common operations where the URL *is* the query/target, a one-step method is more natural.

2. **Wasted platform data**: Apify already returns mentions (Instagram), hashtags (TikTok), post types (Instagram), and platform IDs (Twitter, TikTok) — but we throw them away during mapping.

3. **Page links not surfaced**: Callers that need links from a page must re-parse HTML or regex the markdown. The archive already has the raw HTML at fetch time — it should extract links then.

## Proposed Solution

### Phase 1: Shorthand methods on Archive

Add convenience methods directly on `Archive` that compose `source()` + the content method:

```rust
// modules/rootsignal-archive/src/lib.rs (on Archive impl)

impl Archive {
    pub async fn search(&self, query: &str) -> Result<ArchivedSearchResults, ArchiveError> { ... }
    pub async fn page(&self, url: &str) -> Result<ArchivedPage, ArchiveError> { ... }
    pub async fn feed(&self, url: &str) -> Result<ArchivedFeed, ArchiveError> { ... }
    pub async fn posts(&self, url: &str, limit: u32) -> Result<Vec<Post>, ArchiveError> { ... }
}
```

Each just calls `self.source(url).await?.method().await` internally. No new logic, pure sugar.

**Files:**
- [x] `modules/rootsignal-archive/src/lib.rs` — add shorthand methods on `Archive`

### Phase 2: Richer Post metadata

Add new fields to `Post` and `InsertPost`, map them from each platform service.

#### New fields on Post

```rust
// modules/rootsignal-common/src/types.rs
pub struct Post {
    // ... existing fields ...
    pub mentions: Vec<String>,         // @mentioned accounts
    pub hashtags: Vec<String>,         // #hashtags
    pub media_type: Option<String>,    // "image", "video", "carousel", "reel", etc.
    pub platform_id: Option<String>,   // native ID on the platform
}
```

#### What each platform provides natively

| Field | Instagram | Twitter/X | TikTok | Reddit | Facebook |
|-------|-----------|-----------|--------|--------|----------|
| mentions | `mentions: Vec<String>` | parse from text | parse from text | parse from text | parse from text |
| hashtags | parse from caption | parse from text | `hashtags: Vec<TikTokHashtag>` | — | parse from text |
| media_type | `post_type` field | infer from attachments | always "video" | "text" (no media) | infer from attachments |
| platform_id | `short_code` | `id` | `id` | — | — |

**Fallback extraction**: For platforms that don't provide mentions/hashtags natively, regex-parse from text:
- Mentions: `@[\w.]+`
- Hashtags: `#[\w]+`

This keeps mapping simple — use native data when available, fall back to regex.

#### Database changes

New migration `005_post_metadata.sql`:

```sql
ALTER TABLE posts ADD COLUMN mentions TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE posts ADD COLUMN hashtags TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE posts ADD COLUMN media_type TEXT;
ALTER TABLE posts ADD COLUMN platform_id TEXT;
```

Use Postgres arrays for mentions/hashtags — they're simple string lists, not complex objects.

**Files:**
- [x]`modules/rootsignal-archive/migrations/005_post_metadata.sql` — new columns
- [x]`modules/rootsignal-archive/src/store.rs` — update `InsertPost`, `insert_post`, `get_posts`
- [x]`modules/rootsignal-common/src/types.rs` — add fields to `Post`
- [x]`modules/rootsignal-archive/src/source_handle.rs` — map new fields through from `InsertPost` to `Post`
- [x]`modules/rootsignal-archive/src/services/instagram.rs` — map `mentions`, `post_type` → `media_type`, `short_code` → `platform_id`, parse hashtags from caption
- [x]`modules/rootsignal-archive/src/services/twitter.rs` — parse mentions + hashtags from text, `id` → `platform_id`
- [x]`modules/rootsignal-archive/src/services/tiktok.rs` — map `hashtags`, parse mentions from text, `id` → `platform_id`, media_type = "video"
- [x]`modules/rootsignal-archive/src/services/reddit.rs` — parse mentions + hashtags from text, media_type = "text"
- [x]`modules/rootsignal-archive/src/services/facebook.rs` — parse mentions + hashtags from text

### Phase 3: Page links extraction

Add `links: Vec<String>` to `ArchivedPage`. Extract all links from raw HTML at fetch time using the existing `extract_links_by_pattern` approach, but without a pattern filter.

```rust
// modules/rootsignal-common/src/types.rs
pub struct ArchivedPage {
    // ... existing fields ...
    pub links: Vec<String>,  // all links found in the page
}
```

#### Database changes

In the same `005_post_metadata.sql` migration:

```sql
ALTER TABLE pages ADD COLUMN links TEXT[] NOT NULL DEFAULT '{}';
```

#### Link extraction

Generalize `extract_links_by_pattern` in `links.rs` to also support extracting all links (no pattern filter). Call it during page fetch, store the result.

**Files:**
- [x]`modules/rootsignal-archive/migrations/005_post_metadata.sql` — add links column to pages
- [x]`modules/rootsignal-archive/src/links.rs` — add `extract_all_links(html, base_url) -> Vec<String>`
- [x]`modules/rootsignal-archive/src/store.rs` — update `InsertPage`, `insert_page`, `get_page`
- [x]`modules/rootsignal-common/src/types.rs` — add `links` to `ArchivedPage`
- [x]`modules/rootsignal-archive/src/source_handle.rs` — call link extraction in `PageRequest::send()`, wire through

### Phase 4: Mention/hashtag text parsing utility

Shared utility for regex-extracting mentions and hashtags from post text. Used by platforms that don't provide them natively.

```rust
// modules/rootsignal-archive/src/text_extract.rs

/// Extract @mentions from text. Returns deduplicated, lowercased usernames without the @ prefix.
pub fn extract_mentions(text: &str) -> Vec<String> { ... }

/// Extract #hashtags from text. Returns deduplicated, lowercased tags without the # prefix.
pub fn extract_hashtags(text: &str) -> Vec<String> { ... }
```

**Files:**
- [x]`modules/rootsignal-archive/src/text_extract.rs` — new module with `extract_mentions` and `extract_hashtags`
- [x]`modules/rootsignal-archive/src/lib.rs` — add `mod text_extract`

## Acceptance Criteria

- [x]`archive.search(query).await`, `archive.page(url).await`, `archive.feed(url).await`, `archive.posts(url, limit).await` work as one-step alternatives
- [x]`Post` has `mentions`, `hashtags`, `media_type`, `platform_id` fields populated from each platform
- [x]Instagram posts include native `mentions` and `post_type` → `media_type`
- [x]TikTok posts include native `hashtags`
- [x]All platforms fall back to regex parsing for mentions/hashtags when native data isn't available
- [x]`ArchivedPage` has `links: Vec<String>` populated at fetch time
- [x]Migration runs cleanly against existing data (all new columns have defaults)
- [x]Build passes with no new warnings

## References

- Archive API: `modules/rootsignal-archive/src/lib.rs`
- SourceHandle: `modules/rootsignal-archive/src/source_handle.rs`
- Store layer: `modules/rootsignal-archive/src/store.rs`
- Common types: `modules/rootsignal-common/src/types.rs`
- Apify types: `modules/apify-client/src/types.rs`
- Existing link extraction: `modules/rootsignal-archive/src/links.rs`
- HTML→markdown: `modules/rootsignal-archive/src/readability.rs`
- Prior refactor plan: `docs/plans/2026-02-22-refactor-archive-content-type-api-plan.md`
