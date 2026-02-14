---
date: 2026-02-14
topic: clustering-infrastructure
---

# Clustering Infrastructure for Deduplication

## What We're Building

A polymorphic clustering system that groups duplicate listings and entities without deleting or merging source data. Instead of inline dedup during ingestion, listings and entities flow in freely. A batch clustering step groups items that refer to the same real-world thing using embedding similarity (pgvector). Downstream consumers (search, heat maps, detail pages) simply pick the representative item from each cluster.

## Why This Approach

The original framing was "dedup infrastructure" — detect and merge duplicates during normalization. But rootsignal is fundamentally a search engine over publicly available data. Search engines don't deduplicate, they rank. Duplicates from multiple sources are actually a positive signal (higher confidence, richer provenance).

Clustering gives us the grouping benefit without the complexity of merge/un-merge logic, human review queues, or inline dedup slowing down ingestion. It's read-side infrastructure — the write path stays dumb and fast.

If we ever want a synthesized canonical record per cluster, that's an additive layer on top, not a prerequisite.

## Key Decisions

- **Cluster, don't merge**: Source data is never modified or deleted. Clustering is a read-side concern.
- **Polymorphic**: Same clustering infrastructure handles listings, entities, and any future clusterable type. Follows existing codebase patterns (taggables, locationables, noteables).
- **Batch, not inline**: Clustering runs as a periodic post-hoc job, not during normalization. Keeps ingestion fast.
- **Stored representative**: Each cluster stores a representative_id for fast reads. Recomputed when a new item joins the cluster.
- **Embedding similarity via pgvector**: Natural fit — listings already have 1536-dim embeddings. Entities will need embeddings added.

## Schema

```sql
CREATE TABLE clusters (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    cluster_type TEXT NOT NULL,        -- 'listing', 'entity'
    representative_id UUID NOT NULL,   -- best item in the cluster
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE cluster_items (
    cluster_id UUID NOT NULL REFERENCES clusters(id),
    item_id UUID NOT NULL,
    item_type TEXT NOT NULL,            -- 'listing', 'entity'
    similarity_score FLOAT,            -- distance to representative
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (cluster_id, item_id, item_type)
);
```

## How It Works

1. **Ingestion** — Listings/entities created as normal. No dedup checks beyond existing fingerprint fast-path.
2. **Clustering job** — Periodically scans unclustered items. For each, queries pgvector for nearest neighbors above a similarity threshold. Either assigns to an existing cluster or creates a new one.
3. **Representative selection** — When a new item joins a cluster, recompute the representative based on heuristic (highest confidence, most complete fields, most recent).
4. **Search** — Query returns one result per cluster (the representative). Detail view shows all cluster items as provenance.
5. **Heat map** — One pin per cluster. Cluster size drives intensity.
6. **Synthesis (future)** — Optional step that merges cluster items into a richer canonical record.

## Open Questions

- Similarity threshold for clustering — needs tuning. Start conservative (high threshold) and loosen.
- Representative selection heuristic — confidence score? Field completeness? Recency? Some combination?
- Should the existing fingerprint dedup in normalize.rs stay as a cheap fast-path, or remove it entirely in favor of clustering?
- Entity embeddings — entities don't have embeddings yet. Need to add them or cluster entities on structured field similarity instead.
- Clustering job cadence — after each ingestion run? Hourly? On-demand?

## Next Steps

→ `/workflows:plan` for implementation details
