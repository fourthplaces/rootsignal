# Source-Based Actor Identity

**Date:** 2026-03-08
**Status:** Draft

## Problem

Actors are created via two conflicting paths:

1. **Dedup (source-based):** `resolve_actor_inline()` creates actors keyed by source URL canonical_key for owned platforms. This is correct.
2. **Actor extractor (name-based):** LLM extracts names from signal text, creates actors with `canonical_key = name.to_lowercase().replace(' ', "-")`. This is brittle — same org spelled differently = duplicate actors, no source anchor.

The name-based path has no connection to the source that produced the signal. Two Instagram accounts for the same org create two unrelated actors. There's no merge mechanism in practice.

## Design Principle

**Actor identity = source URL.** An actor is "the entity that owns these sources." Actors only exist when anchored to at least one source. Named entities without source URLs are leads, not actors.

## Current State (What Already Works)

- `resolve_actor_inline()` in dedup creates actors from owned sources (Instagram, Facebook, Twitter, etc.) with `canonical_key` = source canonical_key
- `ActorLinkedToSource` world event and `(Actor)-[:HAS_SOURCE]->(Source)` Neo4j edge exist
- `DuplicateActorsMerged` event and projector logic exist (rewires ACTED_IN + HAS_SOURCE edges)
- Link promotion discovers social handles on pages and promotes them as sources
- Social scrape extracts author name from posts

## Current State (What's Missing)

- No profile bio/description capture from social platforms
- No profile link following for actor enrichment
- No ownership verification when claiming a source for an actor
- No merge trigger on source collision
- No named entity → SERP expansion query path
- `actor_extractor.rs` creates sourceless, name-keyed actors (to be deleted)
- `social_urls` field on ActorNode is never populated (derived from HAS_SOURCE instead)

## The Flywheel

Each scout run adds a little more. No single run needs to be exhaustive.

- **Run 1:** Scrape Instagram source → create actor from profile → extract signals
- **Run 2:** Follow bio link to website → verify ownership → claim source for actor
- **Run 3:** SERP query discovers their Facebook → claim source → signals from both platforms linked to same actor
- **Run 4:** Another actor claims same source → collision detected → merge

## Phases

### Phase 1: Clean Up Name-Based Path

Delete `actor_extractor.rs` and its invocation in `run_enrichment`. The dedup path (`resolve_actor_inline`) already handles source-based actor creation for owned platforms. This removes the conflicting, brittle path.

**Changes:**
- Delete `modules/rootsignal-scout/src/domains/enrichment/activities/actor_extractor.rs`
- Remove `actor_extractor` call from `run_enrichment` handler in `enrichment/mod.rs`
- Remove `find_actor_by_name` from `SignalReader` trait (no longer needed)
- Keep `resolve_actor_inline()` in dedup as the sole actor creation path

**Risk:** Actors that were only discoverable via name extraction (mentioned but not authored) won't be created. This is intentional — they become expansion leads (Phase 4).

### Phase 2: Profile Enrichment

Capture profile metadata from social sources during scraping. The social fetcher already returns author info, but we don't capture bio, profile links, or other metadata.

**Changes:**
- Extend `Post` or add a `ProfileInfo` struct to carry bio, profile_links, avatar_url from social platforms
- Emit profile data through `SocialScrapeCompleted` event
- Update `resolve_actor_inline()` or add enrichment step to populate actor bio from profile
- `social_urls` field becomes derived: query `HAS_SOURCE` edges instead of storing separately

### Phase 3: Source Claiming via Profile Links

Follow outbound links from actor profiles (bio links, website links) and verify ownership to claim additional sources for the actor.

**Changes:**
- New enrichment activity: `profile_link_claimer` (or extend link_promoter)
- For each actor with HAS_SOURCE edges, fetch their source pages' outbound links
- LLM ownership verification: does the destination page belong to the same entity? (name match, bio match, cross-links)
- If verified: emit `ActorLinkedToSource` to claim the source
- If the source is new: emit `SourceDiscovered` first, then claim

### Phase 4: Handle Extraction + Named Entity Expansion

Two sub-paths for entities mentioned in signal text:

**Explicit handles** (`@maboroshimn`, `instagram.com/foo`):
- Extract during scrape/dedup (partially exists via `mentions` on Post)
- Resolve to source URL → follow Path A (source-based actor creation)

**Named entities only** ("City of Minneapolis"):
- Don't create actor
- Queue SERP expansion query: "City of Minneapolis instagram" / "City of Minneapolis twitter"
- Expansion queries become WebQuery sources for future runs
- When a future run scrapes the discovered source → actor created via Phase 1 path

**Changes:**
- Named entity extractor (lighter than current actor_extractor): extracts entity names, does NOT create actors
- Emit expansion queries as `ExpansionQueryQueued` or similar
- Handle resolution: `@handle` → `platform_url()` → `SourceDiscovered` + `ActorLinkedToSource`

### Phase 5: Merge on Source Collision

When `ActorLinkedToSource` would link a source that already belongs to a different actor, trigger a merge.

**Changes:**
- Detection point: projector or handler checks existing HAS_SOURCE edges before creating new one
- If collision: emit `DuplicateActorsMerged { kept_id, merged_ids }` (already exists)
- Merge policy: keep actor with more signals (higher signal_count) or older first_seen
- Projector already handles rewiring ACTED_IN + HAS_SOURCE edges

## Event Model

No new event types needed — the existing events cover all cases:

| Event | Already Exists | Used For |
|-------|---------------|----------|
| `ActorIdentified` | Yes | Actor creation (canonical_key = source URL) |
| `ActorLinkedToSource` | Yes | Claiming a source for an actor |
| `ActorLinkedToSignal` | Yes | Linking actor to authored/mentioned signals |
| `DuplicateActorsMerged` | Yes | Merging actors on source collision |
| `SourceDiscovered` | Yes | New source from profile link following |

## What Gets Deleted

- `actor_extractor.rs` — name-based actor creation
- `find_actor_by_name` on SignalReader trait
- `social_urls` field on ActorNode (derived from HAS_SOURCE edges)
- Actor extraction LLM call and batching logic

## Migration

No data migration needed. Existing name-keyed actors in Neo4j will be orphaned but harmless. Over time, source-based actors will replace them as the system scrapes. A one-time cleanup script could delete actors with no HAS_SOURCE edges if desired.
