# SIMILAR_TO Edges — Post-Creation Signal Similarity

## Context

Vector-based dedup was removed from the signal creation pipeline because it conflated two concerns:

1. **Dedup** — "have I already seen this exact signal?" → simple `(url, node_type)` check
2. **Corroboration** — "do these two signals describe the same real-world event?" → graph relationship

Jamming corroboration into dedup caused a critical bug: signals with different URLs and different node types were being collapsed into a single node because they had high embedding similarity. An Instagram account posting 23 times about the same topic would produce 1 signal instead of 23 distinct signals of different types (Gathering, Resource, Concern, etc.).

## Principle

**Dedup prevents duplicate creation. Similarity is a relationship between things that exist.**

Signals should always be created if they have a unique `(url, node_type)`. Relationships between signals — including similarity, corroboration, and confidence boosting — happen after creation, in the graph.

## Design

### SIMILAR_TO Edge

```cypher
(a:Gathering)-[:SIMILAR_TO {similarity: 0.94, computed_at: datetime()}]->(b:Gathering)
```

- Bidirectional semantics (create both directions or treat as undirected)
- `similarity` score from embedding cosine similarity
- Only created above a threshold (e.g., 0.90)
- Can cross node types: a Gathering and a Resource about the same event are SIMILAR_TO each other

### When to Compute

Option A: **Synthesis domain** — after signals are created, a synthesis handler computes embeddings and finds similar existing signals in the vector index. Creates SIMILAR_TO edges. This is where `compute_similarity` already lives.

Option B: **Enrichment domain** — as part of the enrichment pipeline that already runs per-signal after review.

Option A feels right — synthesis already owns cross-signal analysis.

### Cross-Source Corroboration (Derived)

Corroboration becomes a graph query, not a dedup verdict:

```cypher
MATCH (a)-[:SIMILAR_TO {similarity: s}]->(b)
WHERE s >= 0.92
  AND (a)-[:PRODUCED_BY]->(sa:Source)
  AND (b)-[:PRODUCED_BY]->(sb:Source)
  AND sa.id <> sb.id
RETURN a, b, s
```

Same signal reported by different sources = corroboration. Confidence boost, source diversity metrics, etc. all derive from the graph.

### Same-Source Refresh (No Longer Needed)

With `(url, node_type)` dedup, same-source re-encounters are caught at creation time. If the signal already exists, the projection updates `last_confirmed_active`. No embedding needed.

### Embedding Cache (Removed)

The in-memory `EmbeddingCache` was a workaround for signals created in the same batch not yet being indexed in Neo4j. With `(url, node_type)` dedup this is unnecessary — the URL check is instant and doesn't need embeddings.

## What This Enables

- **Signal count accuracy** — every distinct `(url, node_type)` pair produces a signal
- **Richer graph** — SIMILAR_TO edges make similarity visible and queryable
- **Confidence from graph structure** — corroboration count derived from SIMILAR_TO edges crossing source boundaries
- **Debuggability** — you can see WHY two signals are considered related (the edge exists with a score)
- **Situation weaving input** — SIMILAR_TO edges feed into situation clustering

## Migration

1. Strip Layer 3 (vector dedup) from dedup pipeline
2. Remove `EmbeddingCache`
3. Dedup becomes: Layer 1 (batch title), Layer 2 (URL title), Layer 2.5 (global title+type), Layer 2.75 (URL+type fingerprint)
4. Add SIMILAR_TO edge creation to synthesis domain
5. Derive corroboration metrics from SIMILAR_TO edges in graph
