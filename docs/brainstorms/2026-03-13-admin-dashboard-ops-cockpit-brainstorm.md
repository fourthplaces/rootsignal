---
date: 2026-03-13
topic: admin-dashboard-ops-cockpit
---

# Admin Dashboard: Ops Cockpit

## What We're Building

Replace the current admin dashboard (a data dump of charts and tables) with a focused operational cockpit that answers three questions at a glance: Is the system healthy? Is the data quality good? What does the graph look like right now?

The current dashboard shows vanity metrics (total signals, stacked area charts, top/bottom source tables) that don't drive any action. The new dashboard is for operators — it surfaces errors, quality gaps, and domain trends that require attention.

## Why This Approach

The admin app serves operators and data stewards, not community members. A "situation room" with live maps and signal feeds belongs in the search app. The admin dashboard should be an ops cockpit that tells you whether the system is working and where the data has gaps.

The current dashboard also has stale terminology ("Tensions" instead of "Concerns") and uses the old 5-type taxonomy (missing Condition entirely).

## Design: Three Sections

### Section 1: System Health (top)

Pipeline status cards — one per flow type (scout, coalesce, weave):
- Last run: timestamp, duration, pass/fail
- Error count in last 24h (aggregated across runs: handler_failed, content_fetch_failed, extraction_failed)
- Budget card: spent today / daily limit, with burn rate indicator

### Section 2: Data Quality (middle)

Quality scorecards in a grid:
- Signals missing category (no LLM classification)
- Signals missing confidence score
- Signals missing location (no geocoded location)
- Orphaned signals (not connected to any situation or concern)
- Validation issues (open count, by severity — already wired via validation_issue_summary)
- Dead sources (3+ consecutive empty runs — find_dead_sources exists)

### Section 3: Graph Overview (bottom)

Domain counts with weekly deltas:
- Situations, Concerns, Help Requests, Resources, Announcements, Conditions
- Each shows current count + trend arrow (up/down) with numeric delta vs 7 days ago
- Hottest concerns shortlist (top 5 by cause_heat)

## What Moves Elsewhere

- Signal volume stacked area chart → Data page (signals tab)
- Top/Bottom sources tables → Data page (sources tab)
- Extraction yield bar chart → Workflows page
- Type distribution pie chart → Data page (signals tab)

## Backend Work Needed

1. **New resolver: `systemHealth`** — aggregates failures across recent runs by type (handler_failed, content_fetch_failed, extraction_failed), returns counts + last error message per type. Query the events table grouped by variant for last 24h/7d.

2. **New graph queries for data quality:**
   - `signals_missing_category()` — `MATCH (n:Signal) WHERE n.category IS NULL RETURN count(n)`
   - `signals_missing_confidence()` — same pattern for confidence
   - `signals_missing_location()` — signals with no HELD_AT/AVAILABLE_AT relationship to Location
   - `orphaned_signal_count()` — signals with no relationship to any Situation or Concern

3. **Weekly delta on domain counts:** Compare current type counts vs 7-day-ago snapshot. Can derive from signal_volume_by_day (sum last 7 days) or add a simple historical count query.

4. **Refactor `admin_dashboard` query** to return the new shape (or create a new query and deprecate the old one).

## What Already Exists (No New Backend Work)

- Budget spent/remaining: `budget_status` query
- Run status and stats: `runs` table with error field, stats JSON
- Validation issues: `validation_issue_summary()` in GraphQueries
- Dead sources: `find_dead_sources()` in GraphQueries
- Unmet concerns with cause_heat: `get_unmet_tensions()` in GraphStore
- Type counts: cached in CachedReader
- Scout running status: already queried from runs table

## Key Decisions

- Dashboard is operator-focused, not community-facing
- Three clear sections that each answer a real question
- Charts/tables that don't drive operator action move to contextual pages (Data, Workflows)
- Fix stale terminology (Tensions → Concerns, old 5-type → current 6-type taxonomy)

## Open Questions

- Should the dashboard be global (all regions) or keep the region selector? Leaning global with per-region drill-down.
- How aggressively should we batch the new Neo4j quality queries? Could be one round-trip with UNION or separate cached queries.
- Should dead sources show an inline "deactivate" action, or just link to the source detail page?

## Next Steps

→ `/workflows:plan` for implementation details
