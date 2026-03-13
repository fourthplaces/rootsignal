# Plan: Collapse impl blocks to plain functions

## Philosophy

If a struct just carries immutable dependencies to call `&self` methods on, and it doesn't implement a trait or serve as a type parameter, those methods can be plain functions that take the dependencies as arguments. Less ceremony, same behavior.

## Candidates

### 1. SimilarityBuilder → module functions (LOW RISK)

**File:** `modules/rootsignal-graph/src/similarity.rs`

**Current:** `struct SimilarityBuilder { client: GraphClient }` with 4 methods.

**Why collapse:** Single field, no `&mut self`, no traits, no type params. Constructed once in one handler, immediately called, then dropped.

**Callsites (1):**
- `modules/rootsignal-scout/src/domains/synthesis/mod.rs:99`

**Change:**

```rust
// Before
let similarity = SimilarityBuilder::new(graph_client.clone());
similarity.compute_edges().await

// After
similarity::compute_edges(&graph_client).await
```

**Files to modify:**
| File | Change |
|------|--------|
| `modules/rootsignal-graph/src/similarity.rs` | Delete struct + impl, replace with `pub async fn compute_edges(client: &GraphClient)`, `pub async fn build_edges(client: &GraphClient)`, `pub async fn clear_edges(client: &GraphClient)`. Keep `fetch_all_embeddings` and `write_edge_batch` as private fns taking `&Graph`. |
| `modules/rootsignal-graph/src/lib.rs` | Change `pub use similarity::SimilarityBuilder` to `pub use similarity::{build_similarity_edges, compute_similarity_edges, clear_similarity_edges}` (or just `pub mod similarity` re-export) |
| `modules/rootsignal-scout/src/domains/synthesis/mod.rs:99` | Replace `SimilarityBuilder::new(gc).compute_edges()` with `rootsignal_graph::similarity::compute_edges(&gc)` |

---

### 2. ResponseMapper (scout version) → module functions (LOW RISK)

**File:** `modules/rootsignal-scout/src/domains/synthesis/activities/response_mapper.rs`

**Current:** `struct ResponseMapper<'a> { graph: &'a GraphReader, api_key, bbox fields }` with 2 public + 1 private method.

**Why collapse:** Borrows `&GraphReader`, holds computed bbox config. No traits, no type params. Constructed once in one handler, called once, dropped.

**Callsite (1):**
- `modules/rootsignal-scout/src/domains/synthesis/mod.rs:137-144`

**Change:**

```rust
// Before
let response_mapper = ResponseMapper::new(&graph, api_key, lat, lng, radius);
response_mapper.map_responses(&mut out).await

// After
response_mapper::map_responses(&graph, api_key, lat, lng, radius, &mut out).await
```

The bbox computation (`lat_delta`, `lng_delta`) moves into `map_responses` as local variables — it's 2 lines of arithmetic, not worth a separate struct.

`verify_response` stays as a private fn in the module taking `api_key` as an arg.

**Files to modify:**
| File | Change |
|------|--------|
| `modules/rootsignal-scout/src/domains/synthesis/activities/response_mapper.rs` | Delete struct + impl, replace with `pub async fn map_responses(graph: &GraphReader, api_key: &str, center_lat: f64, center_lng: f64, radius_km: f64, events: &mut Events)`. Keep `verify_response` as private fn. Keep `ResponseMappingStats` and `ResponseVerdict` as-is. |
| `modules/rootsignal-scout/src/domains/synthesis/mod.rs:137-144` | Replace constructor + method call with direct fn call |

---

### 3. ResponseMapper (graph version) → DELETE (ZERO RISK)

**File:** `modules/rootsignal-graph/src/response.rs`

**Current:** Old copy of ResponseMapper that writes directly to Neo4j (not event-sourced).

**Why delete:** Zero imports anywhere in code. The scout version replaced it. Dead code.

**Files to modify:**
| File | Change |
|------|--------|
| `modules/rootsignal-graph/src/response.rs` | Delete file |
| `modules/rootsignal-graph/src/lib.rs` | Remove `pub mod response` |

---

### 4. GraphClient → type alias (MEDIUM RISK, HIGH TOUCH)

**File:** `modules/rootsignal-graph/src/client.rs`

**Current:** `struct GraphClient { pub(crate) graph: Graph }` with `connect()` and `inner()`.

**Why collapse:** Pure newtype wrapper. `inner()` just returns `&self.graph`. Every consumer does `self.client.graph.execute(...)` or `self.client.graph.run(...)` — the wrapper adds nothing.

**Blast radius:** 46 files reference `GraphClient`. This is the highest-touch change.

**Approach — phased:**

**Phase 4a:** Make `graph` field `pub` instead of `pub(crate)`, add a `pub async fn connect_graph(uri, user, password) -> Result<Graph>` free function. Keep the struct temporarily as `pub struct GraphClient { pub graph: Graph }` so existing code still compiles.

**Phase 4b:** Migrate consumers one module at a time from `client.graph.execute(...)` to just `graph.execute(...)` — passing `&Graph` directly. This is mechanical but touches many files. Do it module-by-module:
1. `rootsignal-graph` internal (reducer, reader, writer, pipeline, similarity, etc.)
2. `rootsignal-scout`
3. `rootsignal-scout-supervisor`
4. `rootsignal-api`
5. `rootsignal-replay`

**Phase 4c:** Once no code uses `GraphClient`, delete `client.rs`, replace with a `pub async fn connect_graph(...)` in `lib.rs`. Update re-exports.

**Decision:** Phase 4 is optional. The payoff (removing one layer of indirection) is real but the touch count is high. Recommend doing phases 1-3 first, then 4 only if the pattern feels worth pursuing after seeing how 1-3 land.

---

## Execution order

1. **Delete dead `response.rs`** — zero risk, pure cleanup
2. **Collapse `SimilarityBuilder`** — 1 callsite, clean module boundary
3. **Collapse scout `ResponseMapper`** — 1 callsite, same pattern
4. **GraphClient** — defer to separate PR if desired

## Verification

Each step:
- `cargo build -p rootsignal-graph` (or relevant crate)
- `cargo build -p rootsignal-scout`
- `cargo test -p rootsignal-graph` (if tests exist for these)
- Grep for old type names to confirm no references remain
