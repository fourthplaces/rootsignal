---
title: "feat: Community Signal Scoring"
type: feat
date: 2026-02-17
---

# Community Signal Scoring

## Overview

Replace raw `corroboration_count` with `source_diversity` (unique entity sources) and `external_ratio` (fraction of evidence from non-self sources) on signal nodes. This surfaces genuinely community-corroborated signals over self-promoted ones.

**The problem:** A single org posting 30 times inflates `corroboration_count` to 30, making their self-promotion rank equally with signals confirmed by 5 independent organizations. Data from the live graph confirms this — Habitat for Humanity has corroboration=30 from just 2 accounts (own Facebook + own Instagram), while ICE-related signals have genuine cross-org attention.

## Problem Statement

`corroboration_count` increments +1 for every duplicate match regardless of source identity. The three corroboration call sites in `scout.rs` (exact title match at line 1051, embedding cache match at line 1127, graph vector match at line 1160) all call `writer.corroborate()` which blindly increments the counter. Entity resolution exists (`EntityMapping` in `sources.rs`) but is only used at the Story level for `entity_count` and velocity — never at the signal level.

## Proposed Solution

### Design Decisions

1. **Keep `corroboration_count`** — it's useful as a raw "total evidence" metric and removing it touches 12+ locations across 6 files for no gain. Add `source_diversity` and `external_ratio` alongside it.

2. **Compute at corroboration time, store on node** — eager computation avoids expensive read-time graph traversals. On each corroboration, resolve the new source's entity, check if it's already represented, and update counts.

3. **Deduplicate entity resolution first** — extract `resolve_entity()` into a shared location before adding a third call site.

4. **Reddit/aggregator limitation: accept for now** — Reddit posts from `r/Minneapolis` resolve to `reddit.com` as a single entity. This is a known limitation. Individual community voices on aggregator platforms are collapsed. A future enhancement could store post-level author identity on Evidence nodes, but that's out of scope.

5. **Display `source_diversity` in UI** — show "Confirmed by N independent sources" instead of raw corroboration count.

### Scoring

`source_diversity` is the integer count of unique resolved entities across a signal's evidence. `external_ratio` is `(evidence_from_non_self_entities) / (total_evidence_count)`, where "self" is the entity resolved from the signal's own `source_url`.

No composite "heat score" formula needed at the signal level — `source_diversity` as a sortable integer is sufficient. Story energy already has its own formula.

## Technical Approach

### Phase 1: Extract Shared Entity Resolution

**Goal:** Single source of truth for `resolve_entity()`.

Currently duplicated in:
- `modules/rootsignal-scout/src/sources.rs:30-60` — used by scout during extraction
- `modules/rootsignal-graph/src/cluster.rs:397-427` — used by clusterer for story metrics

**Changes:**
- `modules/rootsignal-common/src/types.rs` — add `EntityMapping` struct and `resolve_entity()` function (move from `sources.rs`)
- `modules/rootsignal-scout/src/sources.rs` — re-export from common, remove local impl
- `modules/rootsignal-graph/src/cluster.rs` — remove `EntityMappingRef` and local `resolve_entity()`, use shared version

### Phase 2: Add Properties to NodeMeta and Graph

**Goal:** `source_diversity` and `external_ratio` exist on signal nodes.

**Changes:**
- `modules/rootsignal-common/src/types.rs` — add to `NodeMeta`:
  ```rust
  pub source_diversity: u32,    // unique entity sources
  pub external_ratio: f32,      // 0.0-1.0, fraction of non-self evidence
  ```
- `modules/rootsignal-graph/src/writer.rs` — update all 5 `create_*` functions (lines 45-341) to write new properties. Initial values: `source_diversity: 1, external_ratio: 0.0`
- `modules/rootsignal-graph/src/reader.rs` — read new properties in `row_to_node()` (line 935), defaulting to `1` and `0.0` for nodes missing the property (backwards compat)
- `modules/rootsignal-graph/src/migrate.rs` — add migration index: `CREATE INDEX ON :Ask(source_diversity)` etc.

### Phase 3: Update Corroboration to Track Diversity

**Goal:** On each corroboration, update `source_diversity` and `external_ratio`.

**Changes:**
- `modules/rootsignal-graph/src/writer.rs` — modify `corroborate()` (line 568):
  - New signature: `corroborate(node_id, node_type, now, new_source_entity: &str)`
  - After incrementing `corroboration_count`, query existing evidence entities:
    ```cypher
    MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
    WHERE n.id = $id
    RETURN DISTINCT ev.source_url AS url
    ```
  - Resolve each URL to entity, compute unique count and external ratio
  - Update node: `SET n.source_diversity = $diversity, n.external_ratio = $ratio`
- `modules/rootsignal-scout/src/scout.rs` — at all 3 corroboration call sites (lines 1051, 1127, 1160):
  - Resolve the new source URL's entity using shared `resolve_entity()`
  - Pass entity string to `corroborate()`
- `modules/rootsignal-scout/src/scout.rs` — add `entity_id` to `CacheEntry` in embedding cache so cache-hit corroborations can pass entity without re-resolving

### Phase 4: Backfill Migration

**Goal:** Existing signals get correct `source_diversity` and `external_ratio`.

**Changes:**
- `modules/rootsignal-graph/src/migrate.rs` — add backfill function:
  - For each signal node, traverse `SOURCED_FROM` edges
  - Collect evidence `source_url` values
  - Resolve entities, compute diversity and external ratio
  - Update node properties
  - Signals with 0 or 1 evidence nodes: `source_diversity = 1, external_ratio = 0.0`

### Phase 5: Update Web Display

**Goal:** UI shows source diversity instead of raw corroboration count.

**Changes:**
- `modules/rootsignal-web/src/main.rs` — add `source_diversity` and `external_ratio` to `NodeView` struct (line 740) and JSON API responses (lines 414, 852)
- `modules/rootsignal-web/src/templates.rs` — change "N sources" badge to use `source_diversity`: "Confirmed by N independent source(s)" (lines 75-77, 122, 196)
- Template files (`node_detail.html`, `nodes.html`) — update display

## Acceptance Criteria

- [x] `resolve_entity()` lives in one place and is used by scout, clusterer, and corroboration
- [x] New signals get `source_diversity: 1, external_ratio: 0.0` on creation
- [x] Corroboration from the same entity (e.g., org's Facebook + Instagram) does NOT increase `source_diversity`
- [x] Corroboration from a different entity DOES increase `source_diversity` and adjusts `external_ratio`
- [x] `corroboration_count` still increments on every corroboration (raw count preserved)
- [x] Existing signals are backfilled with correct values from their Evidence nodes
- [x] Web UI displays "N independent sources" using `source_diversity`
- [x] Signals missing new properties (old data) default gracefully to `1` / `0.0`

## Known Limitations

- **Reddit/aggregator collapse:** All Reddit posts from a subreddit resolve to `reddit.com` as one entity. Individual community voices are not differentiated. Future: store author identity on Evidence nodes.
- **Entity mapping coverage:** Unmapped sources fall back to domain. New orgs without entity mappings get domain-level resolution, which is reasonable but imperfect.
- **No composite heat formula:** `source_diversity` is used as a raw integer for sorting. A weighted formula combining diversity, confidence, and recency could come later but is YAGNI for now.

## References

- Brainstorm: `docs/brainstorms/2026-02-17-community-signal-scoring-brainstorm.md`
- Corroboration logic: `modules/rootsignal-scout/src/scout.rs:1042-1185`
- Entity resolution (scout): `modules/rootsignal-scout/src/sources.rs:30-60`
- Entity resolution (cluster): `modules/rootsignal-graph/src/cluster.rs:397-427`
- Corroborate function: `modules/rootsignal-graph/src/writer.rs:568-595`
- Story energy formula: `modules/rootsignal-graph/src/cluster.rs:514-582`
- NodeMeta: `modules/rootsignal-common/src/types.rs:233-250`
- Learning: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md` — use `Option<T>` over defaults for data quality
