---
date: 2026-03-03
topic: analytics-projection
---

# Analytics Projection Dashboard

## What We're Building

An operator-facing analytics dashboard in the admin-app, projected from the event log onto dedicated Postgres tables. The navigation hierarchy is top-down: **Overview (aggregate health) → Run detail → Source detail → Signal detail**.

Primary use case: debugging runs, tuning source weights, understanding system health — spotting anomalies from the overview and drilling down.

## Why This Approach

**Approach A (chosen): Projection handlers** — new tables maintained by seesaw event handlers, same pattern as Neo4j projection and `scout_runs`.

Considered alternatives:
- **Materialized views over events table** — fights the architecture, JSONB queries get slow at 100k+ events, refresh timing is a separate concern.
- **Hybrid** — unnecessary complexity at current scale.

Projection handlers fit because: data stays fresh in real-time, tables are replayable (drop + replay = rebuild), no new infrastructure, consistent with existing patterns.

## Schema (3 New Tables)

### `run_sources` — what each source did in each run

| Column | Type | Notes |
|--------|------|-------|
| run_id | TEXT | FK to scout_runs |
| source_key | TEXT | canonical_key (e.g., "web:https://...") |
| source_role | TEXT | tension / response / discovery |
| urls_scraped | INT | |
| signals_produced | INT | |
| signals_deduplicated | INT | |
| scrape_failed | BOOL | |
| scrape_unchanged | BOOL | content hash matched |
| duration_ms | INT | scrape wall time |

### `source_stats` — rolling health per source (upserted on each run)

| Column | Type | Notes |
|--------|------|-------|
| source_key | TEXT | PK |
| source_role | TEXT | |
| total_runs | INT | times scheduled |
| total_signals | INT | lifetime yield |
| total_scrapes | INT | |
| consecutive_empty | INT | runs with 0 signals |
| last_signal_at | TIMESTAMPTZ | |
| last_scraped_at | TIMESTAMPTZ | |
| avg_signals_per_run | FLOAT | |
| active | BOOL | |

### `run_signals` — individual signals per run (for drill-down)

| Column | Type | Notes |
|--------|------|-------|
| run_id | TEXT | |
| signal_id | UUID | |
| signal_type | TEXT | gathering, concern, etc. |
| source_key | TEXT | which source produced it |
| title | TEXT | signal title/summary |
| category | TEXT | if classified |
| confidence | FLOAT | if scored |
| created_at | TIMESTAMPTZ | |

## Dashboard Views

### 1. Overview Dashboard (entry point)
- Signals/day sparkline (from `run_signals` grouped by date)
- Source health distribution (from `source_stats`: active vs. empty vs. deactivated)
- Recent runs table with quick stats (from `scout_runs`)
- Error rate trend (failed scrapes / total scrapes over time)

### 2. Run Detail (click a run)
- Run metadata: duration, region, timestamps
- Source breakdown table: each source in the run, what it yielded
- Signal list: all signals produced, grouped by type
- Phase timeline (optional later — from pipeline events)

### 3. Source Detail (click a source)
- Source metadata: key, role, weight, cadence
- Performance over time: signals per run as a chart
- Run history: every run this source participated in
- Signal list: what this source has ever produced

### 4. Signal Detail (click a signal)
- Full signal data (may already exist in Neo4j graph explorer)
- Provenance: which source, which run, causal chain

## Key Decisions
- **Projection handlers over mat views**: consistency with existing architecture, real-time freshness, replayable
- **Postgres (not a separate analytics DB)**: right choice at medium scale (100k–1M events)
- **Admin-app (React/TS)**: new pages in existing app, not a separate tool
- **Pre-computed tables**: JSONB aggregation over events table is too slow at this scale

## Open Questions
- Which events map to `run_sources` rows? Likely `SourceScraped` + signal creation events correlated by source_key + run_id
- Should `source_stats` be maintained incrementally by handlers, or recomputed periodically from `run_sources`?
- Phase timeline view — worth building early or defer?
- Charts library choice for admin-app (Recharts? Tremor? Something else?)

## Next Steps
→ `/workflows:plan` when ready to implement
