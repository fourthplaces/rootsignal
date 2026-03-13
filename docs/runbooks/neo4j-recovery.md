# Neo4j Graph Recovery

The Neo4j graph is a derived projection from the Postgres event store. When the graph falls out of sync — due to Neo4j downtime, a projector bug, or schema changes in the `GraphProjector` — it can be rebuilt from the event store.

## When to use

- Neo4j was unreachable during a scout run (events persisted but projection failed)
- You deployed a change to `GraphProjector` that affects how events are projected (new properties, renamed fields, restructured relationships)
- Graph data looks wrong or inconsistent and you suspect projection drift
- Neo4j was restored from backup and is behind the event store

## How to detect drift

Check whether the event store is ahead of the graph:

```sql
-- Latest event in Postgres
SELECT MAX(seq) AS latest_seq, COUNT(*) AS total_events FROM events;
```

Compare against node counts in Neo4j:

```cypher
MATCH (n) RETURN labels(n)[0] AS label, count(*) AS count ORDER BY count DESC;
```

If the event count is significantly higher than expected graph nodes, projection has fallen behind.

## Recovery: full rebuild

The `rootsignal-replay` binary wipes Neo4j and replays all events from the event store.

### 1. Dry run (safe, read-only)

```bash
cargo run -p rootsignal-replay
```

Prints the total event count and exits. Requires `DATABASE_URL`, `NEO4J_URI`, `NEO4J_USER`, `NEO4J_PASSWORD` in env or `.env`.

### 2. Execute rebuild

```bash
cargo run -p rootsignal-replay -- --commit
```

This will:
1. Run Neo4j schema migrations (constraints, indexes)
2. Wipe all Neo4j nodes in 10k-node batches
3. Replay every event through `GraphProjector::project()` in 1000-event batches
4. Log progress every batch and print a final summary (applied / no-op / errors)

### 3. Verify

After replay, spot-check:

```cypher
// Total nodes by type
MATCH (n) RETURN labels(n)[0] AS label, count(*) AS count ORDER BY count DESC;

// Situations with signals
MATCH (sig)-[:PART_OF]->(s:Situation) RETURN s.title, count(sig) ORDER BY count(sig) DESC LIMIT 10;

// Sources with scrape counts
MATCH (s:Source) RETURN s.canonical_key, s.scrape_count ORDER BY s.scrape_count DESC LIMIT 10;
```

## Recovery: partial replay

If you know the exact seq where projection stopped (e.g., from error logs), you can avoid a full wipe by calling the library directly:

```rust
use rootsignal_graph::GraphProjector;
use rootsignal_events::EventStore;

// Replay from a specific sequence (no wipe)
projector.replay_from(&store, seq_start).await?;
```

This is faster but assumes all events before `seq_start` are already correctly projected.

## Failure modes

| Scenario | Impact | Recovery |
|----------|--------|----------|
| Neo4j down during scout run | Events persist in Postgres; graph stale | Full rebuild after Neo4j recovers |
| Projector bug (wrong MERGE) | Graph has incorrect data for affected events | Fix projector, full rebuild |
| Neo4j disk full | Projection fails mid-batch; some events projected, some not | Clear space, full rebuild |
| Replay interrupted mid-way | Partial graph (some nodes exist, others don't) | Re-run `--commit` (idempotent MERGE handles duplicates) |

## Important notes

- The event store is always the source of truth. Neo4j can be rebuilt at any time.
- Replay is idempotent — running it twice produces the same result (all projections use MERGE).
- Replay does NOT affect Postgres data, snapshots, or aggregate state.
- During replay, the API will serve stale/incomplete graph data. Consider draining traffic first.
- `RUST_LOG=replay=debug` shows per-event detail if you need to diagnose projection errors.
