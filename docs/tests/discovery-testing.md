# Discovery Engine Testing Playbook

Tests for the LLM-powered curiosity engine (`discovery.rs`). Verifies that the discoverer builds correct briefings, generates useful queries, deduplicates against existing sources, and degrades gracefully when budget or LLM is unavailable.

## Prerequisites

```bash
docker compose up -d memgraph web
MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"
```

The graph should have signals from at least one full scout run (with tensions and stories). If not, run a city first.

## 1. Discovery Runs and Creates Query Sources

Run scout and verify the discovery phase executes after scraping and investigation.

```bash
cargo run --bin scout -- --city twincities 2>&1 | grep -iE "discovery|curiosity|gap|LLM discovery"
```

**Expected output includes:**
- `Cold start detected` (on a sparse graph) OR `LLM discovery: created query source` lines
- `Discovery: actors=N, links=N, gaps=N, skipped=N`
- `gaps` > 0 on graphs with tensions

**Fail criteria:**
- `Failed to build discovery briefing` — graph queries for tensions/stories/sources are broken
- `LLM discovery failed, falling back to mechanical` — Anthropic API key invalid or Haiku error
- `gaps=0` on a graph with 3+ tensions and at least 1 story — curiosity engine is not generating queries

## 2. Query Sources Exist in Graph

```bash
echo "MATCH (s:Source {source_type: 'web_query', discovery_method: 'gap_analysis'})
RETURN s.canonical_value AS query, s.gap_context AS context, s.weight AS weight, s.active AS active
ORDER BY s.created_at DESC LIMIT 10;" | $MG
```

**Pass:** Returns rows with:
- `query` contains the city name or neighborhood names
- `context` starts with `Curiosity:` (LLM-driven) or `Tension:` (mechanical fallback)
- `weight` = 0.3 (initial default)
- `active` = true

**Fail:** Empty results after a run with 3+ tensions and stories — source creation is broken.

## 3. Cold-Start Fallback

On a new city with fewer than 3 tensions and no stories, discovery should fall back to mechanical templates.

```bash
# Seed a minimal city with few signals
cargo run --bin scout -- --city portland 2>&1 | grep -iE "cold start|mechanical|gap analysis"
```

**Pass:** Logs show `Cold start detected (< 3 tensions, 0 stories), using mechanical discovery`.

**Fail:** LLM discovery attempted on a nearly empty graph — `is_cold_start()` logic is broken.

Verify mechanical queries are tension-derived:

```bash
echo "MATCH (s:Source {source_type: 'web_query', city: 'portland'})
RETURN s.canonical_value AS query, s.gap_context AS context LIMIT 10;" | $MG
```

**Pass:** Queries follow the template `{what_would_help} resources services {city_slug}` and `gap_context` starts with `Tension:`.

## 4. Query Deduplication

Run the same city twice. Discovery should not create duplicate query sources.

```bash
# Count before
echo "MATCH (s:Source {source_type: 'web_query', discovery_method: 'gap_analysis'})
RETURN count(s) AS count;" | $MG

# Run again
cargo run --bin scout -- --city twincities 2>&1 | grep -i "discovery"

# Count after
echo "MATCH (s:Source {source_type: 'web_query', discovery_method: 'gap_analysis'})
RETURN count(s) AS count;" | $MG
```

**Pass:** Second run shows `skipped` > 0 in Discovery stats. Count increases only by genuinely new queries (or stays the same).

**Fail:** Count doubles — substring dedup in `discover_from_curiosity` is broken.

Also verify no exact canonical_key duplicates:

```bash
echo "MATCH (s:Source {source_type: 'web_query'})
WITH s.canonical_key AS ck, count(*) AS c WHERE c > 1
RETURN ck, c;" | $MG
```

Must return empty.

## 5. Feedback Loop — Successes and Failures

The briefing sent to the LLM must include past discovery performance so it can learn from what worked and avoid what failed.

```bash
# Check that discovery sources have been scraped and have production stats
echo "MATCH (s:Source {discovery_method: 'gap_analysis'})
WHERE s.signals_produced > 0 OR s.consecutive_empty_runs > 0
RETURN s.canonical_value AS query, s.signals_produced AS produced,
       s.consecutive_empty_runs AS empty_runs, s.weight AS weight
ORDER BY s.signals_produced DESC LIMIT 10;" | $MG
```

**Pass:** After 2+ runs, some discovery sources show `produced > 0` (successes) and some show `empty_runs > 0` (failures). These are what the briefing feeds back to the LLM.

**Fail:** All discovery sources show `produced=0, empty_runs=0` — the scraper is not processing discovered query sources.

## 6. Strategy Performance Tracking

```bash
echo "MATCH (s:Source {discovery_method: 'gap_analysis'})
WITH s.gap_context AS ctx,
     CASE WHEN s.signals_produced > 0 THEN 1 ELSE 0 END AS success
RETURN
  CASE WHEN ctx CONTAINS 'unmet_tension' THEN 'unmet_tension'
       WHEN ctx CONTAINS 'signal_imbalance' THEN 'signal_imbalance'
       WHEN ctx CONTAINS 'emerging_thread' THEN 'emerging_thread'
       WHEN ctx CONTAINS 'novel_angle' THEN 'novel_angle'
       ELSE 'other' END AS gap_type,
  count(*) AS total,
  sum(success) AS successful
ORDER BY total DESC;" | $MG
```

**Pass:** Multiple gap types appear. `unmet_tension` should generally have the highest success rate (it targets concrete needs).

**Fail:** All queries have the same gap_type — the LLM is not diversifying its strategy.

## 7. Budget Exhaustion Graceful Degradation

Set a minimal budget and verify discovery falls back to mechanical.

```bash
# Run with SCOUT_BUDGET_CENTS=1 to exhaust budget quickly
SCOUT_BUDGET_CENTS=1 cargo run --bin scout -- --city twincities 2>&1 | grep -iE "budget exhausted|mechanical|discovery"
```

**Pass:** Logs show `Skipping LLM discovery (budget exhausted), falling back to mechanical`.

**Fail:** LLM discovery attempted despite exhausted budget — budget check in `discover_from_curiosity` is broken.

## 8. Actor-Derived Source Discovery

The discoverer also creates sources from Actor domains and social URLs found in signals.

```bash
echo "MATCH (s:Source {discovery_method: 'signal_reference'})
RETURN s.canonical_value AS source, s.source_type AS type, s.gap_context AS context
ORDER BY s.created_at DESC LIMIT 10;" | $MG
```

**Pass:** Returns rows with `context` starting with `Actor:` and source types matching the URL (web, instagram, etc.).

**Fail:** Empty after runs with signals mentioning organizations — `discover_from_actors` is not finding actor domains.

## 9. Max Query Cap

The system caps LLM-suggested queries at 7 per run.

```bash
# Count discovery sources created in the last hour
echo "MATCH (s:Source {discovery_method: 'gap_analysis'})
WHERE s.created_at > datetime() - duration('PT1H')
RETURN count(s) AS recent_queries;" | $MG
```

**Pass:** `recent_queries` <= 7 (MAX_CURIOSITY_QUERIES).

**Fail:** More than 7 — the `.take(MAX_CURIOSITY_QUERIES)` cap is broken.

## 10. Query Quality Spot-Check

Pull the 5 most recent discovery queries and evaluate them manually.

```bash
echo "MATCH (s:Source {source_type: 'web_query', discovery_method: 'gap_analysis'})
RETURN s.canonical_value AS query, s.gap_context AS context
ORDER BY s.created_at DESC LIMIT 5;" | $MG
```

**What to look for:**
- Queries include the city name or specific neighborhoods
- Queries target organizations, programs, or resources — not news articles
- Queries are specific ("affordable housing waitlist programs Minneapolis") not vague ("housing crisis")
- Gap context explains the reasoning clearly
- No queries duplicating what curated web sources already cover

**What is NOT a problem:**
- Mechanical fallback queries being less creative than LLM queries — that's expected
- Some queries producing zero signals — discovery is exploratory by nature

## 11. Pipeline Unchanged

Discovery is additive and non-fatal. Existing validation must still pass.

```bash
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

**All existing checks must still pass.**

## 12. Engagement-Aware Discovery

Verify that engagement data (corroboration_count, source_diversity, cause_heat) is surfaced in the briefing and used to prioritize discovery queries without gating.

### Unit tests (automated)

```bash
cargo test -p rootsignal-scout --lib discovery
```

Key tests:
- `briefing_engagement_shown_for_tensions` — engagement line appears under each tension
- `briefing_engagement_zero_still_shown` — zero-engagement tensions still appear (no gate)
- `briefing_high_engagement_tensions_first` — all tensions present regardless of engagement
- `mechanical_fallback_sorts_by_engagement` — high-engagement sorts first, low-engagement still present
- `cold_start_ignores_engagement` — cold start works with zero-engagement tensions

### Manual: engagement data flows from graph

```bash
# Check that tensions have engagement properties
echo "MATCH (t:Tension) WHERE t.corroboration_count > 0 RETURN t.title, t.corroboration_count, t.source_diversity, t.cause_heat LIMIT 5;" | $MG
```

### Manual: new single-source tensions still get discovery queries

```bash
# Create a tension with zero engagement
echo "CREATE (t:Tension {title: 'Test zero engagement', severity: 'high', what_would_help: 'test resources', last_confirmed_active: datetime(), corroboration_count: 0, source_diversity: 0, cause_heat: 0.0});" | $MG

# Run scout and verify it creates a query for this tension
cargo run --bin scout -- --city twincities 2>&1 | grep -i "test zero engagement\|gap analysis"

# Cleanup
echo "MATCH (t:Tension {title: 'Test zero engagement'}) DELETE t;" | $MG
```

### Manual: high-engagement tensions get priority slots

```bash
# Check that discovery queries reference high-engagement tensions
echo "MATCH (s:Source {source_type: 'web_query', active: true})
      WHERE s.gap_context CONTAINS 'Tension:'
      RETURN s.canonical_value, s.gap_context
      ORDER BY s.created_at DESC LIMIT 10;" | $MG
```

Verify that tensions with higher corroboration/source_diversity appear in earlier query slots.

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| `Failed to build discovery briefing` | Graph query syntax error or missing node properties |
| `LLM discovery failed` | Anthropic API key invalid, rate limited, or Haiku structured output regression |
| `Cold start detected` on a full graph | `is_cold_start()` thresholds wrong (needs >= 3 tensions AND >= 1 story) |
| Duplicate query sources | Substring dedup comparing wrong case, or canonical_key MERGE broken |
| All queries same gap_type | LLM system prompt not encouraging diversity, or briefing missing sections |
| Discovery sources never scraped | Scheduler not picking up WebQuery sources, or Serper API key missing |
| `gaps=0` with tensions present | `get_unmet_tensions` query returning empty, or `get_active_sources` dedup too aggressive |
| Actor sources not discovered | `get_actors_with_domains` query broken, or actors don't have domain/social_url properties |
| Engagement data all zeros | Tensions haven't been through investigation yet, or `corroboration_count`/`source_diversity` properties not being set |
| `cause_heat` always 0.0 | Clustering hasn't run yet — cause_heat is only set during periodic clustering |
| Low-engagement tensions never get queries | Bug in sort logic or mechanical fallback — engagement should rank, not gate |
