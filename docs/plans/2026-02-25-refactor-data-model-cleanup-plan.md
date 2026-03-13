---
title: Data Model Cleanup
type: refactor
date: 2026-02-25
---

# Data Model Cleanup

## Overview

An audit of the Neo4j data model surfaced several issues: deprecated layers still carrying code weight, an incomplete edge type enum, confusing naming, denormalized bloat, and untyped state machines. This plan captures each issue, investigation findings, and resolution.

---

## Issue 1: Remove deprecated Story layer

**Status:** Ready to execute
**Severity:** High — ~5000+ lines across 12+ files
**Resolution:** Delete all Story code. Replace `get_story_landscape()` in source_finder with situation equivalent.

### Findings

Story has **zero active writers** in production. `create_story`, `update_story`, `link_signal_to_story` are only called from tests. `StorySynthesizer.synthesize()` is never invoked. Situation + SituationWeaver has fully replaced it.

**Still actively read:**
- `source_finder.rs` calls `get_story_landscape(8)` for discovery briefing context → migrate to `get_situation_landscape()`
- GraphQL: 5+ query resolvers, 2 mutations (tag/untag), StoryBySignalLoader, TagsByStoryLoader
- Admin UI: StoryDetailPage, story fields in SignalDetailPage, DashboardPage arc/category charts, GraphNode colors, FilterSidebar

**Fully removable:**
- Writer: 14 methods (~400 lines)
- Reader: 18 methods (~500 lines)
- CachedReader: 17 methods + 5 cache data structures (~600 lines)
- GraphQL: types, resolvers, loaders, mutations (~300 lines)
- Admin frontend: StoryDetailPage, queries, mutations, component refs
- `story_metrics.rs` — entire file (story_status, story_energy, parse_recency)
- `synthesizer.rs` — entire file (StorySynthesisResponse, compute_arc, parse_category)
- Types: StoryNode, StoryBrief, StoryGrowth, StorySynthesis, StoryArc, StoryCategory
- Tests: triangulation_test.rs, story tests in litmus_test.rs, bbox_scoping_test.rs
- Migration: keep Story→LegacyStory relabel (rollback safety), remove Story constraint/index creation

### Acceptance Criteria

- [ ] Remove all Story writer methods from writer.rs
- [ ] Remove all Story reader methods from reader.rs and cached_reader.rs
- [ ] Remove all Story GraphQL resolvers, mutations, loaders, types
- [ ] Remove admin frontend Story pages, queries, mutations, component refs
- [ ] Remove `story_metrics.rs` and `synthesizer.rs`
- [ ] Remove Story type definitions from types.rs (StoryNode, StoryArc, StoryCategory, StorySynthesis, StoryBrief, StoryGrowth)
- [ ] Remove Story tests (triangulation_test.rs, story litmus tests, bbox story tests)
- [ ] Migrate `source_finder.rs` to use `get_situation_landscape()` instead of `get_story_landscape()`
- [ ] Remove ClusterSnapshot (see Issue 7)
- [ ] `cargo check` and `cargo test` pass
- [ ] Admin app builds

---

## Issue 2: EdgeType enum is incomplete

**Status:** Ready to execute
**Severity:** Medium
**Resolution:** Complete the enum + add Display/FromStr impls. Do NOT change runtime usage yet (edge_type flows as String through API).

### Findings

EdgeType is **never imported or matched** anywhere outside its own definition. The API uses `edge_type: String` throughout. All 22 edge types are created/read as raw Cypher strings.

The enum is pure documentation today. But it *should* be the source of truth for what edges exist in the graph.

**Missing variants:** ProducedBy, PartOf, EvidenceOf, HasDispatch, Cites, HasSource, HasSchedule, DiscoveredFrom

### Acceptance Criteria

- [ ] Add 8 missing variants to EdgeType
- [ ] Add `Display` impl mapping to SCREAMING_SNAKE_CASE (Neo4j names)
- [ ] Add `FromStr` impl for the reverse
- [ ] Remove any Story-only edge types after Issue 1 (EvolvedFrom, Contains if only Story uses it)
- [ ] Verify no compile errors

---

## Issue 3: SOURCED_FROM vs PRODUCED_BY naming

**Status:** Needs discussion
**Severity:** Medium — naming confusion, not structural

### Findings

- `SOURCED_FROM`: Signal → Citation (URL where signal was found, with snippet/relevance/confidence)
- `PRODUCED_BY`: Signal → Source (tracked source that generated the signal during a scout run)

These are genuinely different concepts:
- Citation = "this URL contains evidence for this signal" (epistemic provenance)
- Source = "this tracked feed/page produced this signal" (operational provenance)

### Options

**Option A: Rename SOURCED_FROM → HAS_CITATION**
- Clearer: signal "has" a citation, not "sourced from" it
- Matches the target node name (Citation)
- ~40 Cypher query edits + migration to relabel existing edges

**Option B: Rename PRODUCED_BY → SCRAPED_FROM**
- More descriptive of the operational relationship
- Fewer references (~10 queries)
- But "scraped" is too implementation-specific

**Option C: Leave as-is, document the distinction**
- Zero migration cost
- Add comments to EdgeType enum explaining each

**Recommendation:** Option A (HAS_CITATION) is clearest. But this is a graph migration — needs a `MATCH ()-[r:SOURCED_FROM]->() CREATE ... DELETE r` step.

- [ ] Decide on rename direction
- [ ] If renaming, plan graph migration

---

## Issue 4: RESPONDS_TO vs DRAWN_TO

**Status:** Keep as-is ✓
**Severity:** Resolved — the distinction is real

### Findings

These are **intentionally separate** edge types with a real semantic distinction:

| | RESPONDS_TO | DRAWN_TO |
|---|---|---|
| **Created by** | ResponseFinder | GatheringFinder |
| **Meaning** | Instrumental response (legal aid, food shelf) | Community formation (vigil, solidarity meal) |
| **`gathering_type` property** | Never present | Always present |
| **Semantic** | Problem-solving | People drawn together |

The distinction was a deliberate refactoring from a property-based hack (`gathering_type IS NOT NULL` on RESPONDS_TO) to separate edge types. A migration already exists that split them.

They're queried together ~95% of the time because both indicate "tension is active," but consumers who care (map apps, dashboards) use the `edge_type` field to distinguish them.

**No action needed.**

---

## Issue 5: Denormalized signal properties

**Status:** Done
**Severity:** Low-Medium

### Findings

| Property | Must keep? | Reason |
|---|---|---|
| **cause_heat** | Yes | O(n²) batch computation, can't be computed on read |
| **source_diversity** | Yes | Used in sorting, discovery, story energy. Cheap to derive but read frequently |
| **channel_diversity** | Yes | Used in cause_heat multiplier. Same rationale as source_diversity |
| **corroboration_count** | Keep for now | Used in severity inference and source ranking. Could be `count(SOURCED_FROM)-1` but read hot |
| **external_ratio** | Remove candidate | Display only — not used in sorting or filtering |
| **freshness_score** | Remove candidate | Set once at extraction, never updated. Dead/incomplete feature |
| **mentioned_actors** | Remove | Dead code — always `Vec::new()`, never computed |
| **author_actor** | Remove | Dead code — always `None`, never computed |
| **was_corrected** | Keep | Audit trail, minimal overhead |
| **corrections** | Keep | Audit trail, minimal overhead |

### Acceptance Criteria

- [x] Remove `mentioned_actors` from NodeMeta and all readers (always empty)
- [x] Remove `author_actor` from NodeMeta and all readers (always None)
- [x] Remove `freshness_score` (always 1.0, never decayed, never used in any logic)
- [x] Remove `external_ratio` (computed but never exposed in GraphQL or used in any sorting/filtering)
- [x] Refactor actor creation from `meta.author_actor` to `ActorContext` (cleaner — uses URL identity directly)
- [x] Simplify `compute_source_diversity` to return `u32` (was `(u32, f32)` with external_ratio)
- [x] Clean up dead `is_owned_source` function and unused macro

---

## Issue 6: ScoutTask phase_status is an untyped string

**Status:** Ready to execute
**Severity:** Low

### Findings

13 valid states forming a strict DAG:

```
idle → running_bootstrap → bootstrap_complete
     → running_scrape → scrape_complete
     → running_synthesis → synthesis_complete
     → running_situation_weaver → situation_weaver_complete
     → running_lint → lint_complete
     → running_supervisor → complete
```

Error recovery: any `running_*` → `idle`. Stale detection: running states >5min auto-reset to idle.

The CAS-based transition function works well, but the valid states are string literals scattered across 6 workflow files.

### Acceptance Criteria

- [ ] Create `PhaseStatus` enum in rootsignal-common
- [ ] Add Display/FromStr mapping to the string values (for Neo4j compatibility)
- [ ] Change `ScoutTask.phase_status` from `String` to `PhaseStatus`
- [ ] Update `transition_task_phase_status` to accept `PhaseStatus` or keep string interface at DB boundary
- [ ] Replace string literals in workflow files with enum variants

---

## Issue 7: ClusterSnapshot — retire

**Status:** Ready to execute (bundle with Issue 1)
**Severity:** Low — 100% dead code

### Findings

ClusterSnapshot is **completely unused**. Four writer methods exist but are **never called** from any production or test code:
- `create_cluster_snapshot()` — 0 callers
- `get_snapshot_count_7d_ago()` — 0 callers
- `get_snapshot_entity_count_7d_ago()` — 0 callers
- `get_snapshot_gap_7d_ago()` — 0 callers

The velocity-tracking concept was absorbed by Situation's real-time temperature metrics (`entity_velocity`, `response_coverage`).

### Acceptance Criteria

- [ ] Remove ClusterSnapshot struct from types.rs
- [ ] Remove 4 ClusterSnapshot methods from writer.rs
- [ ] Remove ClusterSnapshot constraint from migrate.rs
- [ ] Remove ClusterSnapshot import from writer.rs

---

## Execution Order

1. **Issue 1 + 7**: Remove Story + ClusterSnapshot (biggest impact, clears the most dead code)
2. **Issue 5**: Remove dead signal properties (mentioned_actors, author_actor, freshness_score, external_ratio)
3. **Issue 2**: Complete EdgeType enum (clean up after Story edges removed)
4. **Issue 6**: Type phase_status
5. **Issue 3**: Rename SOURCED_FROM (requires graph migration — defer if not urgent)

Issue 4 is resolved (no action).

## References

- NodeType enum: `modules/rootsignal-common/src/types.rs:62`
- EdgeType enum: `modules/rootsignal-common/src/types.rs:1581`
- Migration: `modules/rootsignal-graph/src/migrate.rs`
- Writer: `modules/rootsignal-graph/src/writer.rs`
- SituationWeaver: `modules/rootsignal-graph/src/situation_weaver.rs`
- Gravity brainstorm: `docs/brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md`
