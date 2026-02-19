# Investigation Framework Testing Playbook

Tests for the Investigation Framework (Phase 2). Verifies that the Investigator selects targets, generates search queries, evaluates evidence, creates Evidence nodes, and respects the 7-day cooldown.

## Prerequisites

```bash
docker compose up -d memgraph
MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"
```

The graph should already have signals from at least one scout run. If not, run a city first to seed data.

## 1. Investigation Runs and Creates Evidence

Run scout for a city and verify the investigation phase executes.

```bash
cargo run --bin scout -- --city twincities 2>&1 | grep -E "investigation|Investigation|Evidence created|targets"
```

**Expected output includes:**
- `Starting investigation phase...`
- `Investigation targets selected count=N` (where N >= 1)
- One or more `Evidence created` lines with `signal_id`, `evidence_url`, `relevance`, `confidence`
- `Investigation: N targets found, N investigated, 0 failed, M evidence created, K web search queries`

**Fail criteria:**
- `No investigation targets found` on a graph with signals — Cypher queries in `find_investigation_targets()` are broken
- `Investigation failed for signal` — LLM extract or web search failing
- `Failed to find investigation targets` — graph connection or query error

## 2. Evidence Nodes Exist in Graph

```bash
echo "MATCH (ev:Evidence) RETURN count(ev) AS evidence_count;" | $MG
```

**Pass:** Count > 0 after at least one investigation run.

Verify evidence is linked to signals:

```bash
echo "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence) RETURN labels(n)[0] AS signal_type, n.title AS title, ev.source_url AS evidence_url, ev.snippet AS snippet LIMIT 10;" | $MG
```

**Pass:** Returns rows with signal titles and evidence URLs from different domains.

**Fail:** Empty results — `create_evidence()` or the `SOURCED_FROM` relationship is broken.

## 3. investigated_at Timestamps Set

```bash
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) AND n.investigated_at IS NOT NULL RETURN labels(n)[0] AS type, n.title AS title, n.investigated_at AS investigated_at LIMIT 10;" | $MG
```

**Pass:** Returns rows with recent `investigated_at` timestamps.

**Fail:** Empty results — `mark_investigated()` is not setting the property.

## 4. 7-Day Cooldown

Run the same city twice in succession. The second run should investigate **different** signals (or find no targets if all have been investigated).

```bash
# First run
cargo run --bin scout -- --city twincities 2>&1 | grep "Investigation:"

# Second run (immediately after)
cargo run --bin scout -- --city twincities 2>&1 | grep "Investigation:"
```

**Pass:** Second run either:
- Investigates a different signal (different `signal_id` in logs)
- Reports `No investigation targets found` (all candidates already investigated)

**Fail:** Same signal investigated twice — the `investigated_at IS NULL OR investigated_at < now - 7d` filter is broken.

Verify cooldown in the graph:

```bash
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) AND n.investigated_at IS NOT NULL AND n.investigated_at > datetime() - duration('PT1H') RETURN n.id AS id, n.title AS title, n.investigated_at AS ts;" | $MG
```

## 5. Per-Domain Dedup (Budget Exhaustion Protection)

No single source domain should monopolize investigation slots.

```bash
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) AND n.investigated_at IS NOT NULL WITH n.source_url AS url RETURN url ORDER BY url;" | $MG
```

**Pass:** Investigated signals come from different source domains.

**Fail:** Multiple investigated signals share the same source domain in a single run — per-domain dedup in `collect_investigation_targets()` is broken.

## 6. Same-Domain Evidence Filtering

Investigation evidence should come from domains different than the signal's source.

```bash
echo "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
WHERE n.investigated_at IS NOT NULL
WITH n.source_url AS signal_source, ev.source_url AS evidence_source
WHERE evidence_source CONTAINS split(split(signal_source, '//')[1], '/')[0]
RETURN signal_source, evidence_source;" | $MG
```

**Pass:** Returns empty — no evidence URLs share the signal's domain.

**Fail:** Evidence from the same domain as the signal — same-domain filter in `investigate_signal()` is broken.

## 7. Sensitivity-Aware Queries

Signals with `sensitivity: "sensitive"` or `"elevated"` should trigger constrained investigation queries (no enforcement actions, legal cases, individual names).

This is best verified by inspecting logs for a sensitive signal:

```bash
# Find a sensitive signal
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) AND n.sensitivity IN ['sensitive', 'elevated'] RETURN n.title, n.sensitivity LIMIT 5;" | $MG
```

If sensitive signals exist and get investigated, verify the log shows the sensitive prompt was used (the queries generated should focus on organizational information, not enforcement actions).

## 8. Budget Limits

The investigator should respect:
- Max 10 web search queries per run
- Max 5 signals investigated per run
- Max 3 queries per signal

```bash
cargo run --bin scout -- --city twincities 2>&1 | grep "Investigation:"
```

**Pass:** `web search queries` <= 10, `investigated` <= 5.

## 9. Multi-City Investigation

Run multiple cities and verify investigation works across all.

```bash
for city in twincities nyc portland; do
  echo "=== $city ==="
  cargo run --bin scout -- --city $city 2>&1 | grep "Investigation:"
  echo ""
done
```

**Pass:** Each city reports investigation stats. No errors.

## 10. Pipeline Unchanged

Investigation is non-fatal and additive. Existing validation must still pass.

```bash
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

**All existing checks must still pass.** Investigation should not affect scraping, extraction, dedup, or storage.

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| `No investigation targets found` (with signals in graph) | Cypher queries not matching — check label names, property filters |
| `Investigation failed for signal` | Haiku API error, malformed structured output, or web search auth failure |
| `Failed to mark signal as investigated` | Node type mismatch or graph write error |
| Same signal re-investigated immediately | `investigated_at` not being set or cooldown filter wrong |
| All targets from same domain | Per-domain dedup `HashSet` logic broken in `collect_investigation_targets()` |
| Evidence from same domain as signal | Same-domain filter in `investigate_signal()` not working |
| `Search query budget exhausted` on first signal | MAX_SEARCH_QUERIES_PER_RUN too high or budget counter not incrementing |
| Zero evidence created despite successful queries | Confidence threshold (0.5) filtering everything — LLM returning low confidence |
