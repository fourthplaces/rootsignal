---
title: Individual Signal Discovery via Instagram Hashtags
type: feat
date: 2026-02-17
---

# Individual Signal Discovery via Instagram Hashtags

## Overview

Add a hashtag discovery loop to the scout that searches Instagram hashtags within geographic boundaries, finds posts containing civic signal, extracts signals from them, and tracks the individuals who posted them as discovered entities for ongoing scraping.

This builds on the entity-centric migration (already shipped) and the source node infrastructure (already in place). The system discovers individuals the same way it discovers web sources — emergently, within geographic constraints, with evidence-based trust accumulation.

## Problem Statement

The system currently scrapes 26 hardcoded org Instagram accounts in the Twin Cities. It cannot find individuals broadcasting civic signal — the volunteer coordinator posting daily about mutual aid, the person asking for food for a family, the individual running a GoFundMe because the org behind them can't be public. These are the most important signals during crises, and the system is blind to them.

## Proposed Solution

A three-phase discovery loop that runs as part of each scout cycle:

1. **Hashtag Search** — Scrape posts from seed hashtags via Apify's `apify/instagram-hashtag-scraper`
2. **Signal Extraction** — Feed posts through the existing LLM extractor, creating Event/Give/Ask/Notice nodes
3. **Entity Discovery** — Record each poster's username as a discovered `Source` node with `source_type: Instagram` and `discovery_method: HashtagDiscovery`

On subsequent runs, discovered individual accounts are scraped alongside curated org accounts. The system grows its individual source list organically.

## Technical Approach

### Phase 1: Apify Hashtag Scraper Integration

**Files:** `modules/apify-client/src/lib.rs`, `modules/apify-client/src/types.rs`

Add the `apify/instagram-hashtag-scraper` actor alongside the existing profile scraper.

**New types in `types.rs`:**
```rust
/// Input for the apify/instagram-hashtag-scraper actor.
pub struct InstagramHashtagInput {
    pub hashtags: Vec<String>,
    pub results_limit: u32,
}
```

The output type is likely the same `InstagramPost` struct — hashtag scraper returns posts with the same schema (caption, owner_username, url, timestamp, etc.). Verify this during implementation.

**New method in `lib.rs`:**
```rust
pub async fn scrape_instagram_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<InstagramPost>>
```

Actor ID: `apify/instagram-hashtag-scraper` — need to find the exact actor ID string. The [Apify listing](https://apify.com/apify/instagram-hashtag-scraper) has it.

**Budget:** Max 3 hashtag searches per scout run. Each returns up to 20 posts. This caps Apify cost while allowing meaningful discovery.

### Phase 2: Seed Hashtags on CityProfile

**File:** `modules/rootsignal-scout/src/sources.rs`

Add `instagram_hashtags: Vec<&'static str>` to `CityProfile` struct (line 1).

Twin Cities seed hashtags:
```rust
instagram_hashtags: vec![
    "MutualAidMPLS", "MutualAidMN", "VolunteerMN",
    "MinneapolisVolunteer", "TwinCitiesMutualAid",
],
```

Other cities: empty `vec![]` for now.

These are starting points. The system expands from here via hashtag extraction (Phase 5).

### Phase 3: Hashtag Discovery in the Scout Pipeline

**File:** `modules/rootsignal-scout/src/scout.rs`

Add a new method `discover_from_hashtags` called from `run_inner()`, after social media scraping (step 4) and before investigation (step 6).

**Flow:**
1. For each seed hashtag (plus any expanded hashtags from the graph — future phase):
   - Call `apify.scrape_instagram_hashtags(&[hashtag], 20)`
   - Group returned posts by `owner_username`
   - For each unique poster:
     - Concatenate their captions (same pattern as existing Instagram scraping)
     - Feed through `self.extractor.extract(&combined_text, &source_url)`
     - If signals are produced:
       - Run through `store_signals()` (dedup, quality, geo-filter)
       - Create a `Source` node: `source_type: Instagram`, `discovery_method: HashtagDiscovery`, `trust: 0.3` (same as existing Instagram trust baseline)
       - The `Source` URL format: `https://www.instagram.com/{username}/`
2. Track stats: `hashtag_posts_found`, `hashtag_signals_extracted`, `accounts_discovered`

**Budget controls:**
- Max 3 hashtag searches per run (configurable constant)
- Max 5 new accounts discovered per run
- Skip usernames that already exist as Source nodes (curated or previously discovered)

### Phase 4: Consume Discovered Instagram Sources

**File:** `modules/rootsignal-scout/src/scout.rs`

The existing `get_active_sources()` call at line 308 loads all Source nodes from the graph, but lines 357-361 only add `SourceType::Web` sources to the scrape list. Discovered Instagram accounts are loaded but ignored.

**Fix:** Add a branch that collects `SourceType::Instagram` sources and feeds them into the social media scrape pipeline alongside curated accounts. The source URL `https://www.instagram.com/{username}/` can be parsed to extract the username.

This closes the loop: hashtag discovery creates Source nodes → next run loads those Source nodes → scrapes the accounts → creates signals.

### Phase 5: DiscoveryMethod Variant

**File:** `modules/rootsignal-common/src/types.rs`

Add `HashtagDiscovery` to the `DiscoveryMethod` enum (line 295):
```rust
pub enum DiscoveryMethod {
    Curated,
    GapAnalysis,
    SignalReference,
    HashtagDiscovery,  // NEW
}
```

Update the Cypher serialization in `writer.rs` to handle the new variant.

### Phase 6: Individual-Voice Extraction Prompt (Optional, Deferred)

**File:** `modules/rootsignal-scout/src/extractor.rs`

The current extraction prompt (line 299) is implicitly org-focused ("preserve organization phone numbers, emails, and addresses"). Individual posts use first-person voice: "I'm delivering groceries Saturday at noon" rather than "We're hosting an event."

**Assessment:** The current prompt is actually signal-type-focused, not org-focused. It describes Events, Gives, Asks, and Notices generically. An individual saying "I need help moving furniture Saturday" maps cleanly to an Ask. The LLM should handle this without prompt changes.

**Defer prompt changes until we see extraction quality on real individual posts.** If individual posts produce bad signals, revisit the prompt. Don't pre-optimize.

## What This Does NOT Include

- **Hashtag expansion from content** — the LLM extracting new hashtags from discovered posts and feeding them back into seed lists. This is Phase 2 of the discovery loop. Start with static seeds, add expansion once we see what the seed discovery produces.
- **Cross-platform entity dedup** — linking an individual's Instagram, GoFundMe, and Facebook as one entity. The EntityMapping infrastructure supports this, but v1 treats each platform presence as a separate Source node. Dedup happens at the signal level (vector similarity), not the entity level.
- **Individual-voice prompt tuning** — deferred until we see real extraction quality.
- **Opt-in/opt-out mechanism** — public is public per design decision. No consent gate beyond posting publicly with civic hashtags.

## Acceptance Criteria

- [ ] Apify hashtag scraper integration works (`scrape_instagram_hashtags` method)
- [ ] CityProfile has `instagram_hashtags` field with Twin Cities seeds
- [ ] Scout runs hashtag discovery and creates signals from individual posts
- [ ] Discovered accounts are persisted as Source nodes with `HashtagDiscovery` method
- [ ] Subsequent scout runs scrape discovered accounts alongside curated accounts
- [ ] Budget controls enforced (max searches, max discoveries per run)
- [ ] Geo-filter applies to hashtag-discovered signals (same as all other signals)
- [ ] `cargo build` clean, `cargo test` passes

## Success Metrics

From the brainstorm:
1. **Discovery works** — the system finds 5-10 individuals broadcasting civic signal in the Twin Cities without anyone manually adding them
2. **Signal quality** — individual-sourced signals are as actionable as org-sourced signals (locations, dates, action URLs, passing quality bar)
3. **Connection works** — a user can see a signal, trace it to a source, and find the individual's Instagram profile

## Files Modified

| File | Changes |
|------|---------|
| `modules/apify-client/src/lib.rs` | Add `INSTAGRAM_HASHTAG_SCRAPER` constant + `scrape_instagram_hashtags` method |
| `modules/apify-client/src/types.rs` | Add `InstagramHashtagInput` struct |
| `modules/rootsignal-common/src/types.rs` | Add `HashtagDiscovery` to `DiscoveryMethod` |
| `modules/rootsignal-scout/src/sources.rs` | Add `instagram_hashtags` to `CityProfile`, populate Twin Cities seeds |
| `modules/rootsignal-scout/src/scout.rs` | Add `discover_from_hashtags` method, consume discovered Instagram sources |
| `modules/rootsignal-graph/src/writer.rs` | Handle `HashtagDiscovery` in Cypher serialization |

## References

- Brainstorm: `docs/brainstorms/2026-02-17-individual-signal-discovery-brainstorm.md`
- Anti-fragile brainstorm: `docs/brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md` (Proxy Signals section, lines 51-62)
- Volunteer coordinator interview: `docs/interviews/2026-02-17-volunteer-coordinator-interview.md`
- Emergent source discovery plan: `docs/plans/2026-02-17-feat-emergent-source-discovery-plan.md`
- Apify Instagram Hashtag Scraper: https://apify.com/apify/instagram-hashtag-scraper
