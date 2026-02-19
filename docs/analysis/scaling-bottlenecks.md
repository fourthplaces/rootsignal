# Scaling Bottlenecks

Known architectural bottlenecks that will need addressing if signal counts grow into the tens of thousands or millions.

## 1. Similarity Computation — O(n²)

**File:** `modules/rootsignal-graph/src/similarity.rs`

**Current approach:** Fetches all signal embeddings into Rust memory, computes all-pairs cosine similarity in a nested loop, writes SIMILAR_TO edges back in batches.

**Breaks at:** ~10K signals becomes slow, ~100K becomes impractical, 1M+ is impossible (500B pair comparisons).

**Memory cost:** 1M signals × 1024 dims × 8 bytes = ~8GB just to hold vectors.

**Mitigation:** Replace the O(n²) loop with Neo4j's vector index KNN queries. The vector indexes already exist (`migrate.rs`), they're just not used for similarity yet. Per-node query:

```cypher
MATCH (n:Event)
CALL db.index.vector.queryNodes('event_embedding', 20, n.embedding)
YIELD node AS neighbor, score
WHERE neighbor <> n AND score >= 0.65
```

This is O(n × K × log n) instead of O(n²).

## 2. Cause Heat Computation — O(n²)

**File:** `modules/rootsignal-graph/src/cause_heat.rs`

**Current approach:** Same pattern as similarity — loads all embeddings, computes all-pairs cosine similarity in memory, sums heat from Tension neighbors above threshold.

**Breaks at:** Same scale as similarity (~10K+ signals).

**Mitigation:** Once SIMILAR_TO edges exist from the vector-index-based similarity step, cause heat becomes a graph propagation problem. Walk existing edges instead of recomputing all-pairs similarity. Options:
- GDS `gds.pageRank` with Tension nodes as heat sources
- Rust-side streaming over SIMILAR_TO edges (avoids GDS dependency)

## 3. Leiden Clustering — GDS Memory Pressure

**File:** `modules/rootsignal-graph/src/cluster.rs`

**Current approach:** Uses `gds.graph.project` to load the SIMILAR_TO subgraph into JVM heap, then runs `gds.leiden.stream`.

**Breaks at:** Millions of nodes with dense SIMILAR_TO edges require 64GB+ JVM heap for the in-memory projection.

**Mitigation:**
- Project only the SIMILAR_TO subgraph (not all node properties)
- Use `gds.leiden.write` instead of `.stream` to avoid pulling all results through the driver
- Partition by city/region so each Leiden run operates on a smaller subgraph
- Long-term: consider moving Leiden into Rust (removes GDS dependency entirely)

## 4. Full-Scan Queries — No Pagination

**Files:** `reader.rs`, `migrate.rs`, various backfill functions

**Current approach:** Queries like `MATCH (n:Event)` with no SKIP/LIMIT or cursor-based pagination.

**Breaks at:** Millions of nodes per label — full scans become multi-second.

**Mitigation:** Add cursor-based pagination (ORDER BY n.id SKIP/LIMIT) or batch processing with `apoc.periodic.iterate` for write-heavy backfills.

## Priority

Not urgent at current scale (single-city, hundreds to low-thousands of signals). Revisit when approaching 10K+ signals per city or multi-city deployments.
