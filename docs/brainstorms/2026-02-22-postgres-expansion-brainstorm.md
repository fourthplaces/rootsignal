---
date: 2026-02-22
topic: postgres-expansion
---

# Expand PostgreSQL as the Home for Operational & Product Data

## What We're Building

Expand PostgreSQL's role from archive-only storage to the default home for all operational and product data in rootsignal. This starts with migrating scout runs/logs off JSON files on disk, and establishes Postgres as the landing zone for new features like user-facing stories.

## Why This Approach

Neo4j is excellent for the signal/situation graph — modeling relationships between entities is its strength. But structured operational data (scout runs, run logs) and product-facing content (stories) are tabular by nature and don't benefit from graph semantics. Postgres is already in the stack powering the archive module, so extending it is a natural expansion rather than a new dependency.

## Database Responsibility Split

| Database | Role | Examples |
|----------|------|----------|
| Neo4j | Signal graph & relationships | Signals, situations, entity connections |
| PostgreSQL | Operational & product data | Archive content, scout runs/logs, stories, future user-facing features |
| Restate | Workflow orchestration state | Durable execution, run coordination |

## Key Decisions

- **Single Postgres database** for archive + scout runs + future product data. Split later only if operational pressure demands it.
- **Neo4j retains exclusive ownership** of the signals/situations graph. No data is being migrated out.
- **Restate continues to own** workflow orchestration state.
- **Routing rule for new features:** "Does this model relationships between entities?" → Neo4j. Everything else → Postgres.

## Open Questions

- Schema design for scout runs and run log events
- Whether to migrate existing JSON run logs or start fresh
- Stories data model (defer until that feature is closer)
- Module boundaries: should scout runs live in `rootsignal-archive` or should there be a shared database layer?

## Next Steps

→ `/workflows:plan` when ready to dig into schema and migration details
