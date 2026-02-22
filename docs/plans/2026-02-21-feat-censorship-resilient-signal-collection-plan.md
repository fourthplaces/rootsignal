---
title: "Censorship-Resilient Signal Collection"
type: feat
date: 2026-02-21
---

# Censorship-Resilient Signal Collection

## Overview

Build an entity-centric signal collection architecture that anchors the scout in real-world civic actors — organizations offering help and people reporting on what's happening. Entities are pinned to locations, have associated social accounts, and are flagged as trusted. When the scout scans a region, it scrapes the social accounts of trusted entities in that geography. Platform search/feeds are retained as a secondary channel with a first-hand filter. A censorship observatory compares entity posts against search visibility.

This addresses three compounding threats: platform censorship (algorithmic suppression, content removal, API restrictions), AI-generated noise flooding search results, and coordinated astroturfing drowning out genuine community voices.

## Problem Statement

Hashtag and keyword queries on social media platforms return walls of political commentary from people with no connection to the issue. Platforms actively suppress civic content. The open web is flooded with AI-generated content. The self-evolving source discovery model assumed a noisy-but-honest web — that assumption no longer holds. Open ingestion from a corrupted environment produces garbage.

The correction: anchor signal collection in entities with real-world presence. Two kinds of entities matter:

1. **Helpers** — orgs, groups, people offering help (mutual aid, legal clinics, food banks, grassroots collectives)
2. **Reporters** — people on the ground reporting what's happening (local journalists, community reporters, activists documenting conditions)

The goal is maximum coverage: find every entity in a region that's actively helping or reporting. If they're offering help or reporting from the ground, they're trusted. Investigation and deeper verification is a future enhancement, not a gate.

## Proposed Solution

Extend the existing ActorNode model to become the anchor for signal collection:

1. **Entities with locations and social accounts** — ActorNodes are pinned to a location and linked to their social accounts (SourceNodes). Entities are flagged as trusted. Managed via admin app / GraphQL mutations.
2. **Entity-driven scraping** — When the scout scans a region, it finds trusted entities in that geography and scrapes their associated social accounts. Entity profile metadata (bio, location) is passed to the LLM extractor as context for signals that don't mention location explicitly.
3. **Search/feeds (secondary)** — Existing platform search continues with a first-hand filter at the LLM extraction layer. Understood as corrupted and noisy.
4. **Censorship observatory** — Compares entity account posts against search visibility. Tracks suppression rates. Surfaces censorship as Tension signals.

## Technical Approach

### Data Model Changes

#### ActorNode — Extended with location + trust + social accounts

The existing `ActorNode` in the graph gains:

```
(ActorNode {
  name: "Navigate MN",
  entity_type: "organization",  // organization | journalist | organizer | collective
  trusted: true,
  location_lat: 44.9778,
  location_lng: -93.2650,
  location_name: "Minneapolis, MN",
  bio: "Immigration legal aid serving Twin Cities families",
  entity_role: "helper"  // helper | reporter | both
})
  -[:HAS_ACCOUNT]-> (SourceNode { url: "https://instagram.com/navigatemn", ... })
  -[:HAS_ACCOUNT]-> (SourceNode { url: "https://twitter.com/navigatemn", ... })
```

Key design points:
- **Location is on the entity, not the source** — solves the profile-location problem. Posts from @navigatemn inherit Minneapolis context even if the post doesn't mention a location.
- **Trust is on the entity, not the source** — an org with 3 social accounts is trusted once. All its sources inherit that trust via the `HAS_ACCOUNT` edge.
- **Trusted by default if offering help or reporting** — the bar is: are they in the arena? Investigation is a future layer.
- **Managed via admin app** — operators add entities through GraphQL mutations, not config files.

#### SourceNode — New DiscoveryMethod variant

```rust
// modules/rootsignal-common/src/types.rs
enum DiscoveryMethod {
    // ... existing variants ...
    EntityAccount,       // Social account linked to a known entity
    SocialGraphFollow,   // Discovered via trusted entity's social graph
}
```

#### New GraphQL mutations (admin app)

```graphql
mutation CreateEntity(
  name: String!
  entityType: EntityType!  # ORGANIZATION | JOURNALIST | ORGANIZER | COLLECTIVE
  entityRole: EntityRole!  # HELPER | REPORTER | BOTH
  trusted: Boolean!
  location: String!        # "Minneapolis, MN" — geocoded on backend via Nominatim
  bio: String
  socialAccounts: [SocialAccountInput!]
) : Entity

input SocialAccountInput {
  platform: SocialPlatform!
  handle: String!
  url: String!
}

mutation AddEntityAccount(entityId: ID!, account: SocialAccountInput!) : Entity
mutation SetEntityTrust(entityId: ID!, trusted: Boolean!) : Entity
```

The backend calls the existing `geocode_location()` function (Nominatim) to resolve the location string into `location_lat`, `location_lng`, and `location_name` — same pattern used by `create_scout_task` and `run_scout`.

### Implementation Phases

#### Phase 1: Entity Infrastructure

**Goal:** Entities can be created in the graph with locations and linked social accounts. The scout finds trusted entities in a region and scrapes their accounts.

**Files to modify:**

- `modules/rootsignal-common/src/types.rs` — Add `DiscoveryMethod::EntityAccount`, `DiscoveryMethod::SocialGraphFollow`. Add `EntityNode` struct (or extend existing `ActorNode`) with `trusted`, `location_lat`, `location_lng`, `location_name`, `bio`, `entity_type`, `entity_role` fields.
- `modules/rootsignal-graph/src/writer.rs` — New methods: `upsert_entity()`, `link_entity_account()`, `find_trusted_entities_in_region(bbox)`. The region query uses the existing `ScoutScope.bounding_box()` to find entities within the geographic scope.
- `modules/rootsignal-api/src/graphql/mutations.rs` — Add `createEntity`, `addEntityAccount`, `setEntityTrust` mutations. `createEntity` accepts a `location: String` and calls the existing `geocode_location()` to resolve lat/lng/name before persisting.
- `modules/rootsignal-scout/src/scheduling/scheduler.rs` — New scheduling path: before standard scheduling, query for trusted entities in the current `ScoutScope`, collect their `HAS_ACCOUNT` source nodes, and schedule them with elevated weight (0.7) and cadence (12h). These are scheduled in both tension and response phases.
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — When scraping an entity's account, pass entity metadata (bio, location_name, location_lat/lng) to the extractor as context. This gives the LLM location fallback when posts don't mention geography.
- `modules/rootsignal-scout/src/pipeline/extractor.rs` — Accept optional `entity_context` parameter. When present, prepend to extraction: "This content is from [entity name], [bio], located in [location]. Use this location as fallback if the post doesn't mention a specific place."

**Acceptance criteria:**
- [ ] Entities created via GraphQL mutation with location + trust flag
- [ ] Social accounts linked to entities via `HAS_ACCOUNT` edge
- [ ] `find_trusted_entities_in_region()` returns entities within a bounding box
- [ ] Scheduler scrapes trusted entity accounts with elevated priority
- [ ] Entity metadata passed to extractor as location/context fallback
- [ ] Entity accounts never auto-deactivated (warning logged instead)

#### Phase 2: First-Hand Filter on Search/Feed Extraction

**Goal:** LLM extraction applies a two-layer filter when processing content from search/feed sources (not entity accounts). Content from entity account scrapes bypasses the filter.

**Files to modify:**

- `modules/rootsignal-scout/src/pipeline/extractor.rs` — Add conditional first-hand filter instructions to the system prompt when source is NOT linked to a trusted entity. Add `is_firsthand: Option<bool>` field to `ExtractedSignal`.
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — Check whether source has a `HAS_ACCOUNT` edge to a trusted entity. If yes, skip filter. If no, apply filter. Post-extraction, drop signals where `is_firsthand == Some(false)`.

**System prompt addition (conditional, for non-entity sources only):**

```
FIRST-HAND FILTER (applies to this content):
This content comes from platform search results, which are flooded with
political commentary from people not directly involved. Apply strict filtering:

For each potential signal, assess: Is this person describing something happening
to them, their family, their community, or their neighborhood? Or are they
asking for help? If yes, mark is_firsthand: true. If this is political commentary
from someone not personally affected — regardless of viewpoint — mark
is_firsthand: false.

Signal: "My family was taken." → is_firsthand: true
Signal: "There were raids on 5th street today." → is_firsthand: true
Signal: "We need legal observers." → is_firsthand: true
Noise: "ICE is doing great work." → is_firsthand: false
Noise: "The housing crisis is a failure of capitalism." → is_firsthand: false

Only extract signals where is_firsthand is true. Reject the rest.
```

**Acceptance criteria:**
- [ ] System prompt includes first-hand filter for non-entity sources only
- [ ] Entity-linked sources bypass the filter
- [ ] `is_firsthand` field added to `ExtractedSignal` as `Option<bool>` (never `unwrap_or`)
- [ ] Signals with `is_firsthand: false` dropped before graph persistence
- [ ] Test with adversarial examples: political noise rejected, first-hand accounts accepted

#### Phase 3: Entity Discovery via Social Graph

**Goal:** Discovery agents identify new entities referenced by trusted entities and add them to the graph for evaluation.

**Files to modify:**

- `modules/rootsignal-scout/src/pipeline/extractor.rs` — Extend `mentioned_actors` to capture social handles with platform context. Add `mentioned_social_accounts: Vec<MentionedAccount>` to `ExtractedSignal`.
- `modules/rootsignal-scout/src/discovery/source_finder.rs` — New method `discover_entities_from_social_graph()` that:
  1. Collects `mentioned_social_accounts` from signals extracted from trusted entity sources
  2. Counts how many independent trusted entities reference each account
  3. Accounts mentioned by 2+ trusted entities become candidate entities
  4. Candidates created as entities with `trusted: false` initially, associated source created with `DiscoveryMethod::SocialGraphFollow`
  5. If the candidate's posts consistently produce valid signals (3+ signals across 2+ runs), flag for operator review to set trusted

**New struct:**

```rust
// modules/rootsignal-common/src/types.rs
pub struct MentionedAccount {
    pub platform: SocialPlatform,
    pub handle: String,
    pub context: String,  // "mentioned in post", "tagged", "retweeted"
}
```

**Expansion boundaries:**
- One hop only — entities referenced by trusted entities, not entities referenced by candidates
- Maximum 5 new candidate entities discovered per scout run
- Minimum 2 independent trusted entity references required

**Acceptance criteria:**
- [ ] `MentionedAccount` captures platform + handle from post text
- [ ] `discover_entities_from_social_graph()` creates candidate entities from multiply-referenced accounts
- [ ] Candidates enter graph as untrusted entities with associated sources
- [ ] One-hop depth limit enforced
- [ ] Max 5 candidates per run, min 2 independent references required
- [ ] Operator notified when candidates show consistent signal production

#### Phase 4: Censorship Observatory

**Goal:** Compare trusted entity posts against platform search results. Track suppression rates. Surface censorship patterns as Tension signals.

**Approach:** Topic-based embedding comparison. After each scout run:
1. Embed all signals extracted from trusted entity account scrapes
2. Embed all search/feed results (including those rejected by first-hand filter)
3. For each topic cluster covered by entity signals, measure what fraction appeared in search
4. Track suppression rate per platform, per topic over time
5. When suppression exceeds threshold, create a Tension signal

**New files:**

- `modules/rootsignal-scout/src/observatory/mod.rs` — Censorship observatory module
- `modules/rootsignal-scout/src/observatory/comparator.rs` — Topic-based comparison logic
- `modules/rootsignal-scout/src/observatory/tension_generator.rs` — Creates Tension signals when threshold crossed

**Files to modify:**

- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — Store lightweight records of search/feed results before first-hand filtering (post URL, platform, timestamp, topic embedding) for censorship comparison
- `modules/rootsignal-graph/src/writer.rs` — New method `record_censorship_measurement()`

**Comparison algorithm (per-signal max-similarity):**

```
For each trusted entity signal S this run:
  best_match = max(cosine_similarity(S.embedding, R.embedding)
                   for R in search_results on same platform)

  if best_match < 0.3:
    S is "suppressed" — not visible in search

For each topic cluster T covered by trusted entity signals this run:
  suppressed_count = count of suppressed signals in T
  total_count = count of entity signals in T
  suppression_rate = suppressed_count / total_count

  if suppression_rate > 0.5 AND total_count >= 3:
    record_measurement(platform, topic, suppression_rate)

    if rolling_average(suppression_rates, window=3_runs) > 0.7:
      create_censorship_tension(platform, topic)
```

**Why max-similarity over mean:** Averaging embeddings washes out specific signals in noisy search results. Per-signal max-similarity detects suppression of individual posts — if a trusted entity's post has no close neighbor in search results, that specific signal is suppressed.

**Phased rollout:**
- Phase 4a: Log comparison data without generating tensions (2 weeks)
- Phase 4b: Review logs, tune threshold, enable tension generation

**Acceptance criteria:**
- [ ] Search/feed results stored (lightweight) before first-hand filtering
- [ ] Topic-based embedding comparison runs after each scout run
- [ ] Suppression rates tracked per platform/topic
- [ ] Rolling average prevents single-run false positives
- [ ] Censorship Tensions created with evidence (suppression measurements)
- [ ] Phase 4a logging runs for 2+ weeks before enabling tension generation

## Alternative Approaches Considered

**Separate watchlist concept** — Rejected. Entities already exist in the graph as ActorNodes. Adding a parallel "watchlist" creates redundant infrastructure. Better to extend the entity model with trust, location, and social account links.

**Config-file-based watchlist** — Rejected. Entities should be managed via admin app / GraphQL mutations, not static files. This supports operator workflows and scales to many regions without repo changes.

**Region field on SourceNode** — Rejected. Sources are region-independent by design. Geography lives on the entity; signals inherit location from entity context or from post content.

**Investigation as a gate for trust** — Rejected for now. The goal is maximum coverage of helpers and reporters. If they're offering help or reporting from the ground, they're in. Investigation is a future enhancement layer.

## Acceptance Criteria

### Functional Requirements

- [ ] Entities created with location, trust flag, and linked social accounts via admin
- [ ] Scout finds trusted entities in region and scrapes their social accounts
- [ ] Entity metadata provides location context for signal extraction
- [ ] First-hand filter applied to search/feed extraction only
- [ ] Social graph expansion discovers new entities from trusted entity interactions
- [ ] Censorship observatory tracks and surfaces suppression patterns

### Non-Functional Requirements

- [ ] Apify cost increase manageable via existing `BudgetTracker`
- [ ] No regression in existing signal extraction quality
- [ ] Censorship detection false positive rate < 10% after tuning period

### Quality Gates

- [ ] Unit tests for first-hand filter (adversarial examples)
- [ ] Integration test for entity lifecycle (create → link accounts → scrape → extract → persist)
- [ ] Observatory comparison logic tested with synthetic data before production

## Dependencies & Prerequisites

- Web archive layer (partially shipping) — preserves evidence when content is censored/deleted
- Existing Apify account-level scraping (works for all 5 platforms)
- Existing ActorNode model in graph
- Existing embedding infrastructure (Voyage AI) for censorship comparison
- Admin app for entity management

## Risk Analysis & Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Apify cost increase from per-account scraping | High | Medium | Budget cap per run, prioritize highest-weight entity sources first |
| False positive censorship detection | High | High | Phased rollout, rolling average, high threshold |
| Entity coverage gaps (missing grassroots groups) | High | Medium | Low trust bar — if offering help, they're in. Discovery expands from trusted entities. |
| First-hand filter rejects valid signal | Medium | Medium | `is_firsthand` as `Option<bool>`, review rejected signals periodically |
| Apify actors blocked by platforms | Medium | High | Multiple actor fallbacks, archive preserves previously fetched content |
| Entity location drift (org moves, journalist covers new area) | Low | Low | Entity location is operator-managed, updatable via admin |

## References & Research

### Internal References

- Brainstorm: `docs/brainstorms/2026-02-21-censorship-resilient-signal-collection-brainstorm.md`
- Vision: `docs/vision/self-evolving-system.md` (updated: corrupted web reality)
- Vision: `docs/vision/editorial-and-signal-inclusion-principles.md` (updated: first-hand principle)
- Gaps: `docs/gaps.md` (signal vs. noise gaps, censorship gaps)
- ActorNode model: existing in Neo4j graph schema
- SourceNode model: `modules/rootsignal-common/src/types.rs:931`
- Apify client: `modules/apify-client/src/lib.rs`
- Signal extraction: `modules/rootsignal-scout/src/pipeline/extractor.rs:423`
- Source discovery: `modules/rootsignal-scout/src/discovery/source_finder.rs:493`
- Scheduling: `modules/rootsignal-scout/src/scheduling/scheduler.rs`

### Related Plans

- `docs/plans/2026-02-17-feat-emergent-source-discovery-plan.md` — Evidence-based trust model
- `docs/plans/2026-02-17-feat-individual-signal-discovery-via-instagram-hashtags-plan.md` — Individual account discovery
- `docs/plans/2026-02-21-feat-web-archive-layer-plan.md` — Immutable record for censorship evidence
- `docs/plans/2026-02-20-feat-demand-driven-scout-swarm-plan.md` — Geographic resilience

### Key Gotchas (from institutional learnings)

- Never `unwrap_or` on LLM extraction — use `Option<T>` for `is_firsthand` and all new fields
- Social URL patterns change — handle both twitter.com and x.com in canonical_value()
- Archive write failure must not fail the scrape — Postgres errors log warnings but still return content
