# Dedup Pipeline

Deduplication is the most critical quality gate. It prevents signal flooding while ensuring corroboration is tracked across sources. The pipeline runs inside the `signals:dedup` handler for each extracted batch.

## Layer Architecture

```
Signal extracted from page
    │
    ├─ Layer 1: Batch title dedup (pre-stash)
    │  (title, type) HashSet — drops duplicates within the same extraction batch
    │  Applied by the caller before the dedup handler runs
    │
    ├─ Layer 2: URL-scoped title dedup
    │  Queries existing titles for the current URL
    │  Drops signals whose normalized title already exists at this URL
    │
    ├─ Layer 2.5: Global exact title+type match
    │  Graph query for (normalized_title, node_type) across all sources
    │  Same URL → Refresh, different URL → Corroborate (similarity = 1.0)
    │
    ├─ Layer 3a: Embedding cache (in-memory cosine similarity)
    │  EmbeddingCache — cross-batch within-run dedup
    │  Thresholds: ≥0.85 entry, ≥0.92 cross-source
    │  Same URL → Refresh, different URL + above threshold → Corroborate
    │
    ├─ Layer 3b: Graph vector index (persistent cosine similarity)
    │  Neo4j vector index query — cross-run dedup
    │  Same thresholds as Layer 3a
    │
    └─ Layer 4: No match → Create
       Signal passed all layers — emit NewSignalAccepted
```

## Verdicts

Each signal receives one of three verdicts:

| Verdict | Event Emitted | Meaning |
|---------|--------------|---------|
| **New** | `NewSignalAccepted` | No match found — create signal, citation, and edges |
| **Cross-source match** | `CrossSourceMatchDetected` | Different source confirms existing signal — add citation, increment corroboration |
| **Same-source refresh** | `SameSourceReencountered` | Same source re-reports existing signal — add citation, confirm freshness |

## Embedding Cache

`EmbeddingCache` (`core/embedding_cache.rs`) is an in-memory cache scoped to a single run. It stores `(embedding, node_id, source_key)` tuples and supports cosine similarity search.

The cache is **not aggregate state** — it is a service on `ScoutEngineDeps`, using interior mutability (`std::sync::RwLock`). This is an intentional exception to the "all state through aggregates" rule because:

- It is transient (run-scoped, not persisted)
- It is deterministic (same inputs produce same cache state)
- It is a performance optimization, not domain state

The `dedup` handler calls `deps.embed_cache.add()` after processing each signal, building up the cache across batches within the run.

## Threshold Rationale

| Threshold | Context | Rationale |
|-----------|---------|-----------|
| 1.0 | Global exact match (Layer 2.5) | Exact title + type — no ambiguity, always acts |
| 0.92 | Cross-source vector (Layer 3a/3b) | High bar — different sources may describe the same event differently. Only very similar content should merge. |
| 0.85 | Same-source vector entry (Layer 3a/3b) | Lower bar — same source re-scraping likely produces minor wording variations of the same signal. |

## Data Flow Through the Sub-Chain

```
batch_title_dedup (Layer 1, pre-handler)
    │
    └─ Filters batch by (normalized_title, node_type) HashSet

dedup handler (Layers 2–4)
    │
    ├─ Reads: deps.store.existing_titles_for_url() → Layer 2 URL-scoped dedup
    ├─ Reads: deps.store.find_by_titles_and_types() → Layer 2.5 global exact match
    ├─ Reads: deps.embed_cache → Layer 3a in-memory vector match
    ├─ Reads: deps.store.find_duplicate() → Layer 3b graph vector match
    │
    ├─ Emits: NewSignalAccepted { node_id, node_type, pending_node }
    │         → aggregator stashes PendingNode in state.pending_nodes
    │         → triggers signals:create handler
    │
    ├─ Emits: CrossSourceMatchDetected { existing_id, similarity }
    │         → triggers signals:corroborate handler
    │
    └─ Emits: SameSourceReencountered { existing_id, similarity }
              → triggers signals:refresh handler

create handler
    │
    ├─ Reads: state.pending_nodes[node_id]
    ├─ Emits: WorldEvent (signal creation) + CitationPublished + SensitivityClassified
    ├─ Emits: SignalCreated { node_id }
    │         → aggregator moves PendingNode to WiringContext
    │         → triggers signals:wire_edges handler
    │
    └─ Writes: deps.embed_cache.add() (via dedup, not create)

wire_edges handler
    │
    ├─ Reads: state.wiring_contexts[node_id]
    └─ Emits: WorldEvent edges (SignalLinkedToSource, ResourceLinked, ActorIdentified, SignalTagged)
```
