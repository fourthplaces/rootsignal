---
date: 2026-03-01
topic: replay-tooling
---

# Replay Tooling for Neo4j Projection Rebuild

## What We're Building

A standalone `rootsignal-replay` binary that rebuilds the Neo4j graph projection from the Postgres event store. The event store is the source of truth; Neo4j is a disposable derived view. When the reducer (GraphProjector) changes — new properties, renamed fields, restructured relationships — replay rebuilds the graph to match the new reducer logic. No Cypher migration scripts needed.

Dry-run by default. `--commit` to execute.

## Why This Approach

**Considered:**

1. **Cypher migrations** — Write one-shot scripts to transform existing Neo4j data. Fast, but creates dual maintenance: the migration and the reducer both encode the same schema. Drift is inevitable.

2. **Replay from events** — Wipe Neo4j, replay all events through the updated reducer. One code path, one truth. The reducer *is* the schema definition.

3. **Event upcasting** — Transform old event payloads at read time so the reducer only handles the latest shape. Complements replay but isn't needed until event schemas actually change.

Replay is the architecturally correct answer. At <100K events, it completes in seconds. Upcasters are a future addition when event shapes evolve.

## Key Decisions

- **Wipe-always**: Every replay wipes Neo4j first. Partial replay onto old-schema data produces inconsistent state. The only correct operation is full rebuild.
- **Dry-run by default**: `rootsignal-replay` shows the plan (event count, estimated time). `--commit` executes. Safe default since this wipes a database.
- **Standalone binary**: Not a dev-cli subcommand. Deployable independently, runnable in CI or on a server.
- **Batch reads**: Use `EventStore::read_from(seq, 1000)` in a loop. No streaming needed at current scale.
- **Fire-and-forget errors**: Same as live projection — log deserialization errors, don't halt. The reducer handles unknown event types as no-ops.
- **Upcaster registry**: Empty to start. Hook exists for future `(event_type, schema_v) → new_payload` transforms. Not implemented until needed.

## Design

### Binary interface

```
rootsignal-replay             # dry-run: count events, show plan
rootsignal-replay --commit    # wipe Neo4j, replay all events for real
```

### Components

**1. Neo4j wipe** — Batched `MATCH (n) WITH n LIMIT 10000 DETACH DELETE n` loop until empty. Avoids OOM on large graphs. Runs before replay.

**2. Event reader** — Paginated loop over `EventStore::read_from(seq, batch_size)`. Advances seq cursor after each batch. Stops when batch returns empty.

**3. Projector** — Existing `GraphProjector::project(&StoredEvent)` called for each event. Already idempotent via MERGE. Already handles unknown types as no-ops.

**4. Progress reporting** — Tracing logs every N events: `Replayed 5000/87432 events (5.7%)`. Final summary: applied/no-op/error counts.

**5. Upcaster registry (stub)** — `Vec<Box<dyn Fn(&str, i16, Value) -> Value>>` applied before projection. Empty initially. Future: register transforms when event schemas change.

### Config

Needs `DATABASE_URL` (Postgres) and `NEO4J_URI/USER/PASSWORD`. No API keys needed — replay doesn't call external services. Use a minimal `Config` variant or just read env vars directly.

### Crate structure

```
modules/rootsignal-replay/
  Cargo.toml          # [[bin]] name = "rootsignal-replay"
  src/
    main.rs           # CLI entry, config, orchestration
    wipe.rs           # Neo4j wipe helper
    upcaster.rs       # Upcaster registry (stub)
```

Dependencies: `rootsignal-events` (EventStore), `rootsignal-graph` (GraphProjector, GraphClient), `rootsignal-common` (Config, event types), `clap`, `tracing`, `sqlx`, `tokio`.

## Open Questions

- Should replay also rebuild Postgres-side derived tables (aggregate snapshots)? Or just Neo4j?
- Should there be a `--labels` filter to only replay events that affect certain node types? (Probably not — keep it simple.)
- When upcasters are needed, should they live in `rootsignal-replay` or in `rootsignal-events` (so the live projection path can also upcast)?

## Next Steps

→ Plan implementation with `/workflows:plan`
