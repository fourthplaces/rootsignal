# BUG: Projection DB read path is split-brain

**Priority: High**
**Status: Open**
**Created: 2026-03-09**

## Problem

Live mode and replay write projected Postgres tables (`scout_runs`, `scheduled_scrapes`) to **different databases**:

- **Live mode**: seesaw engine projections (`scout_runs_projection`, `scheduled_scrapes_projection`) write to the main `DATABASE_URL` pool
- **Replay mode**: `PostgresProjector` writes to a separate `_projection` database

The API server reads from the main `DATABASE_URL` pool. After a replay, the projection DB has the correct rebuilt state but **nobody reads from it**. The main DB's projected tables are stale or inconsistent.

This is the same class of problem we solved for Neo4j with versioned databases (`neo4j.v{version}`) — but we haven't solved it for Postgres projected tables yet.

## Impact

- After replay, `scout_runs` and `scheduled_scrapes` in the main DB are stale
- `is_source_busy` queries read stale data from the main DB
- Scheduled scrapes loop reads stale data from the main DB
- The projection DB rebuild is correct but unused

## Options

1. **Swap on promote**: After replay health checks pass, point the API's read pool at the projection DB (mirrors Neo4j versioned DB pattern)
2. **Unified write target**: Thread a second pool through `ScoutEngineDeps` so live projections also write to the projection DB. API reads from projection DB exclusively.
3. **Single DB, truncate-and-rebuild**: Drop the separate DB. Replay truncates the projected tables in the main DB and rebuilds in-place. Simpler but couples event store and read model.

Option 2 is the cleanest long-term (full read model separation), but Option 3 is pragmatic if we don't need independent scaling.
