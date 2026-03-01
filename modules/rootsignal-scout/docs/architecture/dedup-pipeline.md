# Dedup Pipeline

Deduplication is the most critical quality gate. It prevents signal flooding while ensuring corroboration is tracked across sources. The pipeline runs inside the `signals:dedup` handler for each extracted batch.

## 4-Layer Architecture

```
Signal extracted from page
    │
    ├─ Layer 1: Title match (exact)
    │  Title + type HashSet — catches duplicates within the same scrape batch
    │
    ├─ Layer 2: Embedding cache (in-memory cosine similarity)
    │  EmbeddingCache — cross-batch within-run dedup
    │  Thresholds: 0.85 (same source) / 0.92 (cross-source)
    │
    ├─ Layer 3: Graph vector index (persistent cosine similarity)
    │  Neo4j vector index query — cross-run dedup
    │  Same thresholds as Layer 2
    │
    └─ Layer 4: Same-source refresh
       URL-scoped query — detects re-encounters from the same source
       Lower threshold (0.80) since provenance is known
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
| 0.92 | Cross-source | High bar — different sources may describe the same event differently. Only very similar content should merge. |
| 0.85 | Same source | Lower bar — same source re-scraping likely produces minor wording variations of the same signal. |
| 0.80 | Same-source refresh | Lowest bar — re-encounter from known provenance. Focus is on confirming the signal is still active. |

## Data Flow Through the Sub-Chain

```
dedup handler
    │
    ├─ Reads: ctx.singleton::<PipelineState>() for url_to_canonical_key, pending_nodes
    ├─ Reads: deps.embed_cache for in-memory vector matches
    ├─ Reads: deps.store (SignalReader) for graph vector + title matches
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
