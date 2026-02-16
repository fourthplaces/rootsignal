---
date: 2026-02-15
topic: signal-clustering
---

# Cross-Source Signal Clustering

## What We're Building

Multi-dimension clustering for signals that groups duplicates across sources without merging or deleting data. The primary use case: an organization posts the same event/ask/give on their Facebook, Instagram, and website. Three different sources, three different extractions, three different signals — but the same real-world thing. Clustering groups them and surfaces the best representative.

## Why This Approach

Same-source dedup is already handled by LLM-matched extraction (signal refresh). But cross-source duplication is a different problem — signals arrive from independent pipelines with no shared identity. Embedding similarity + entity matching solves this at the read layer, consistent with the existing philosophy: "search engines rank, not deduplicate."

The clustering infrastructure already exists for listings/entities. Adapting it for signals is the natural next step.

## Key Decisions

- **Cluster, don't merge**: Source data is never modified or deleted. Clustering is a read-side concern. False negatives (missed clusters) are harmless; false positives (wrongly grouped) suppress real signals from search.
- **Multi-dimension via cluster_type**: `UNIQUE(cluster_type, item_type, item_id)` — a signal can belong to at most one cluster per type. Enables a Venn diagram model where a signal could be in an entity cluster AND a semantic cluster.
- **Start with entity clustering**: Same org posting across platforms is the immediate problem. Additional cluster types (semantic, location) layer on later without schema changes.
- **Conservative threshold, loosen over time**: Start with a high similarity bar. Missed duplicates are harmless; wrong groupings suppress signals.
- **Best representative in search**: Search shows one result per cluster (the representative). Other cluster members exist as provenance. The existing `cluster_reps` CTE in hybrid search already handles this, scoped by cluster_type.

## Schema Change

Current `cluster_items` constraint:
```sql
UNIQUE(item_type, item_id)  -- one cluster per signal, period
```

New constraint:
```sql
UNIQUE(cluster_type, item_type, item_id)  -- one cluster per signal per dimension
```

Also need to add `'signal'` to the CHECK constraints on `clusters.cluster_type` and `cluster_items.item_type`.

## Entity Clustering Algorithm

The existing composite scoring works well for signals. For the entity cluster type:

- **Embedding similarity** — primary signal, captures semantic equivalence even with different wording
- **Entity match** — `entity_id` on signals; same entity across platforms is a strong clustering indicator
- **Geographic proximity** — via locationables join; same location boosts confidence
- **Temporal proximity** — `broadcasted_at` and schedules; signals posted around the same time about the same thing
- **Signal type match** — two signals should generally be the same type (ask/give/event/informative) to cluster

Weights and thresholds to be tuned, starting conservative.

## What Needs to Change

1. **Migration**: Update CHECK constraints to include `'signal'`, change unique constraint to include `cluster_type`
2. **ClusterItem::unclustered()**: Currently hardcoded to `listings` table — parameterize for `signals`
3. **Clustering activity**: Adapt `cluster_listings.rs` scoring for signal-specific fields (entity_id, signal_type, broadcasted_at)
4. **Search dedup**: Update `cluster_reps` CTE to scope by `cluster_type = 'entity'` (or whichever dimension is active)
5. **Workflow trigger**: Wire clustering to run after signal extraction batches

## Future Cluster Types

The schema supports adding these later without migration:

- **Semantic clustering**: Group signals about the same real-world thing across different entities (e.g., two orgs promoting the same community cleanup)
- **Location clustering**: Group signals in tight geographic proximity about similar topics

These may also overlap with the Activities layer (Layer 3), which is designed to explain *why* signal clusters exist.

## Open Questions

- Exact weight distribution for entity clustering scoring dimensions
- Whether `cluster_type` should be stored on the `clusters` table, the `cluster_items` table, or both (currently on `clusters` — may need denormalization for the unique constraint)
- Clustering job cadence — after each extraction batch? Periodic? On-demand?
- Representative selection heuristic for signals — field completeness, confidence, recency, source tier?

## Next Steps

-> `/workflows:plan` for implementation details
