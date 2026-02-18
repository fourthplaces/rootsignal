---
title: "feat: Cause Heat Cross-Story Signal Boosting"
type: feat
date: 2026-02-17
---

# Cause Heat: Cross-Story Signal Boosting

## Overview

Add a `cause_heat` score to every signal that measures how much independent community attention exists in its semantic neighborhood. A food shelf Ask posted once a week ago rises when the housing crisis is trending — because embedding similarity connects housing and food insecurity. No taxonomy, no LLM calls, just math on existing embeddings.

## Problem Statement

Signals are currently ranked by recency and source diversity at the individual level, and by story energy at the cluster level. But cross-story boosting doesn't exist. A housing Tension trending in Story A doesn't boost a food bank Ask in Story B, even though poverty connects them. Signals from small orgs that showed up for a good cause stay invisible unless they happen to cluster with a hot story.

## Proposed Solution

### Formula

```
cause_heat(signal) = Σ cosine_sim(signal, neighbor) × neighbor.source_diversity
                     for all neighbors where cosine_sim > THRESHOLD
```

Normalize by dividing by the max cause_heat in the batch to produce a 0.0–1.0 score.

### Architecture

Batch computation in Rust during the scout run (after signal storage, before clustering):

1. Load all signals: `MATCH (n) WHERE n:Event OR ... RETURN n.id, n.embedding, n.source_diversity`
2. Compute all-pairs cosine similarity in memory (619 signals × 1024 dims = ~360K dot products, <50ms)
3. For each signal, sum `sim × diversity` for neighbors above threshold
4. Normalize to 0.0–1.0
5. Write back: `MATCH (n {id: $id}) SET n.cause_heat = $heat`

### Phase 1: Add cause_heat computation

**Changes:**
- [x] `modules/rootsignal-graph/src/lib.rs` — add `pub mod cause_heat;`
- [x] `modules/rootsignal-graph/src/cause_heat.rs` — new module:
  - `compute_cause_heat(client, threshold)` — loads embeddings, computes pairwise similarity, writes back
  - `cosine_similarity(a, b) -> f64` — dot product / (norm_a × norm_b)
- [x] `modules/rootsignal-graph/src/migrate.rs` — add index: `CREATE INDEX ON :Event(cause_heat)` etc.

### Phase 2: Wire into scout run

**Changes:**
- [x] `modules/rootsignal-scout/src/scout.rs` — call `compute_cause_heat()` after `store_signals()`, before clustering
  - Or: `modules/rootsignal-scout/src/main.rs` — call after scout.run()

### Phase 3: Add to reader and web display

**Changes:**
- [x] `modules/rootsignal-graph/src/reader.rs` — read `cause_heat` in `row_to_node()` with default 0.0
- [x] `modules/rootsignal-common/src/types.rs` — add `cause_heat: f64` to `NodeMeta`
- [x] `modules/rootsignal-web/src/main.rs` — add to `NodeView`, JSON API, GeoJSON
- [x] `modules/rootsignal-web/src/templates.rs` — show cause heat indicator on signal cards

### Phase 4: Use cause_heat for ranking

**Changes:**
- [x] `modules/rootsignal-graph/src/reader.rs` — update signal listing queries to ORDER BY cause_heat DESC (or a composite score)

## Acceptance Criteria

- [ ] `cause_heat` is computed for all signals with embeddings
- [ ] Signals semantically near high-diversity topics get higher cause_heat
- [ ] A single-post food shelf near a hot housing cluster gets boosted
- [ ] Self-promotion (high corr, low diversity) does NOT inflate cause_heat of neighbors
- [ ] Web UI displays cause_heat and uses it for ranking
- [ ] Computation runs in <1 second for 600 signals

## References

- Brainstorm: `docs/brainstorms/2026-02-17-cause-heat-brainstorm.md`
- Source diversity (dependency): `docs/plans/2026-02-17-feat-community-signal-scoring-plan.md`
- Story energy formula: `modules/rootsignal-graph/src/cluster.rs:525`
- Vector indexes: `modules/rootsignal-graph/src/migrate.rs:121-127`
- Embeddings: 1024-dim Voyage embeddings stored on all signal nodes
