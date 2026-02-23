---
title: "Instagram Stories → Claude Vision Pipeline"
type: feat
date: 2026-02-22
---

# Instagram Stories → Claude Vision Pipeline

## Overview

Instagram stories are where real-time, on-the-ground signal lives — event flyers, protest announcements, community meeting details, calls to action. These are ephemeral (24h) and primarily visual (text overlaid on images), so they're invisible to the current text-only scraping pipeline.

We'll add story fetching via the Apify `louisdeconinck/instagram-story-details-scraper` actor (already accessible with existing API key, tested working), download story images, pass them through Claude vision to extract text content, and merge the results into the existing `SocialPost` pipeline so they flow through extraction like any other post.

## Problem Statement

The scout currently scrapes Instagram posts (captions), but ignores stories entirely. Many organizations — mutual aid groups, legal clinics, community organizers — run campaigns primarily through stories: event flyers, calls to action, urgent announcements, meeting details. This content is visual (text overlaid on images), ephemeral (24h), and never appears in the post feed. The system is blind to a significant channel of real-time community signal.

## Proposed Solution

1. **Fetch active stories** via the Apify `louisdeconinck/instagram-story-details-scraper` actor (ID: `9pQFsbs9nqUI64rDQ`)
2. **Download story images** (filter to `media_type == 1`, skip videos)
3. **Pass images to Claude vision** (Haiku 4.5) to extract all visible text, event details, dates, locations, organizations, and calls to action
4. **Merge extracted text into `SocialPost` pipeline** so stories flow through the same extraction, dedup, and storage as regular posts

## Technical Approach

### 1. `modules/ai-client/src/claude/types.rs` — Add Image content block

Add an `Image` variant to `ContentBlock` for Claude's vision API:

```rust
#[serde(rename = "image")]
Image {
    source: ImageSource,
},
```

With supporting type:
```rust
struct ImageSource {
    #[serde(rename = "type")]
    source_type: String,  // "base64"
    media_type: String,   // "image/jpeg", "image/png", etc.
    data: String,         // base64-encoded image data
}
```

Also add a `WireMessage::user_with_image()` constructor that builds a `Blocks` content with both an image block and a text block (the prompt).

### 2. `modules/apify-client/src/types.rs` — Add story types

```rust
pub struct InstagramStoryInput {
    pub usernames: Vec<String>,
}

pub struct InstagramStory {
    pub id: String,
    pub media_type: i32,           // 1 = image, 2 = video
    pub taken_at: Option<i64>,     // unix timestamp
    pub expiring_at: Option<i64>,  // unix timestamp
    pub user: Option<InstagramStoryUser>,
    pub image_versions2: Option<serde_json::Value>,  // nested CDN URLs
    pub caption: Option<serde_json::Value>,
    pub story_link_stickers: Option<Vec<serde_json::Value>>,
}

pub struct InstagramStoryUser {
    pub username: Option<String>,
    pub full_name: Option<String>,
}
```

Plus `InstagramStory::best_image_url()` helper that extracts the highest-resolution URL from `image_versions2.candidates`.

### 3. `modules/apify-client/src/lib.rs` — Add story scraping method

- Add constant: `INSTAGRAM_STORY_DETAILS_SCRAPER: &str = "9pQFsbs9nqUI64rDQ"`
- Add `scrape_instagram_stories(&self, usernames: &[&str]) -> Result<Vec<InstagramStory>>` following the existing start/poll/fetch pattern
- Re-export new types from `lib.rs`

### 4. `modules/rootsignal-archive/src/fetchers/social.rs` — Add story fetching with vision

Add `fetch_stories()` method to `SocialFetcher`. This needs access to both the Apify client and a Claude instance + HTTP client for image downloading.

Refactor `SocialFetcher` to hold:
- `client: ApifyClient` (existing)
- `claude: Option<Claude>` (new — for vision calls)
- `http: reqwest::Client` (new — for image downloading)

`fetch_stories()` flow:
1. Call `self.client.scrape_instagram_stories(usernames)` → `Vec<InstagramStory>`
2. Filter to `media_type == 1` (images only)
3. For each story image:
   a. Download image bytes from `best_image_url()`
   b. Base64-encode
   c. Send to Claude vision with extraction prompt
   d. Map result to `SocialPost { content: extracted_text, author, url }`
4. Return `Vec<SocialPost>`

Vision prompt: "Extract all visible text, event details, dates, times, locations, organizations, and calls to action from this Instagram story image. If the image contains a flyer or announcement, capture all details. Return the extracted content as plain text."

Use `claude-haiku-4-5-20251001` (same model used everywhere else, supports vision, cheapest).

### 5. `modules/rootsignal-archive/src/archive.rs` — Wire up Claude + HTTP to SocialFetcher

Update `SocialFetcher::new()` to accept `Option<Claude>` and an `http: reqwest::Client`.

In `Archive::new()`, pass the existing `claude` clone and `http_client` clone to `SocialFetcher`.

Add `Archive::fetch_stories()` method that calls through to `SocialFetcher::fetch_stories()` and records the interaction to the store (similar to `fetch_social_profile`).

### 6. `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — Call stories alongside posts

In `run_social()`, after fetching posts for each Instagram source, also fetch stories for the same username. Merge the story-extracted `SocialPost`s into the same `posts` vec before the existing combine → extract flow.

This means stories get the same actor_prefix/firsthand_filter treatment and flow through the same LLM extraction and dedup pipeline.

Only fetch stories for `SocialPlatform::Instagram` sources (no other platform has stories via this actor).

## Files Modified

1. `modules/ai-client/src/claude/types.rs` — Image content block
2. `modules/apify-client/src/types.rs` — Story input/output types
3. `modules/apify-client/src/lib.rs` — Story scraping method + re-exports
4. `modules/rootsignal-archive/src/fetchers/social.rs` — `fetch_stories()` with vision
5. `modules/rootsignal-archive/src/archive.rs` — Wire Claude/HTTP to SocialFetcher, add `fetch_stories()`
6. `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — Merge stories into social pipeline

## Verification

1. `cargo build` — everything compiles
2. Manual test: call `scrape_instagram_stories` with a known active account, verify image download + vision extraction produces readable text
3. Existing tests still pass (`cargo test`)
