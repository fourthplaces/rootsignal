# Supervisor Testing Playbook

Tests for the scout supervisor (`rootsignal-scout-supervisor`). Verifies auto-fix checks, heuristic triage, LLM validation, issue persistence, source penalty feedback, echo detection, and notification delivery.

## Prerequisites

```bash
docker compose up -d memgraph
MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"
```

The graph must have signals and stories from at least one full scout run. Some tests require deliberately seeded bad data.

## 1. Supervisor Runs Successfully

```bash
cargo run --bin supervisor -- --city twincities 2>&1 | tee /tmp/supervisor.log
```

**Expected output includes:**
- `Supervisor checking window from=... to=...`
- `auto_fix(orphaned_evidence=N orphaned_edges=N actors_merged=N empty_signals=N fake_coords=N)`
- `Triage complete total=N`
- `LLM budget summary llm_budget_used=N llm_budget_remaining=N`
- `Echo detection complete scored=N flagged=N`
- `Supervisor run complete.`

**Fail criteria:**
- `Another supervisor is running, exiting` — stale lock. Clean up: `echo "MATCH (l:SupervisorLock) DELETE l;" | $MG`
- Any panic or stack trace
- `Failed to` errors in auto-fix phase (these should be non-fatal but indicate graph issues)

## 2. Lock Acquisition and Release

The supervisor uses a lock to prevent concurrent runs.

```bash
# Run supervisor
cargo run --bin supervisor -- --city twincities &

# Immediately try a second run
cargo run --bin supervisor -- --city twincities
```

**Pass:** Second run shows `Another supervisor is running, exiting`.

After the first run completes, verify the lock is released:

```bash
echo "MATCH (l:SupervisorLock) RETURN count(l) AS locks;" | $MG
```

**Pass:** `locks = 0`.

**Fail:** Lock persists after run — `release_lock()` is broken. Manual cleanup:

```bash
echo "MATCH (l:SupervisorLock) DELETE l;" | $MG
```

## 3. Auto-Fix: Orphaned Evidence

Seed an orphaned Evidence node (no SOURCED_FROM edge pointing to it) and verify it gets cleaned up.

```bash
# Create orphaned evidence
echo "CREATE (ev:Evidence {
  id: 'test-orphan-evidence',
  source_url: 'https://example.com/orphan',
  snippet: 'test orphan',
  content_hash: 'test123'
});" | $MG

# Run supervisor
cargo run --bin supervisor -- --city twincities 2>&1 | grep "orphaned_evidence"

# Verify cleanup
echo "MATCH (ev:Evidence {id: 'test-orphan-evidence'}) RETURN count(ev) AS remaining;" | $MG
```

**Pass:** `orphaned_evidence` > 0 in stats, `remaining = 0` in verification.

## 4. Auto-Fix: Duplicate Actors

Seed two Actor nodes with the same normalized name and verify they get merged.

```bash
# Create duplicate actors
echo "CREATE (a1:Actor {id: 'test-dup-actor-1', name: 'Minneapolis Parks'});
CREATE (a2:Actor {id: 'test-dup-actor-2', name: 'minneapolis-parks'});" | $MG

# Run supervisor
cargo run --bin supervisor -- --city twincities 2>&1 | grep "actors_merged"

# Verify merge
echo "MATCH (a:Actor) WHERE a.id IN ['test-dup-actor-1', 'test-dup-actor-2']
RETURN a.id, a.name;" | $MG
```

**Pass:** `actors_merged` > 0 in stats, only one of the two actors remains.

**Cleanup (if test actors persist):**

```bash
echo "MATCH (a:Actor) WHERE a.id STARTS WITH 'test-dup-actor' DETACH DELETE a;" | $MG
```

## 5. Auto-Fix: Empty Signals

Seed a signal with an empty title and verify it gets deleted.

```bash
echo "CREATE (n:Event {id: 'test-empty-signal', title: '', summary: 'no title'});" | $MG

cargo run --bin supervisor -- --city twincities 2>&1 | grep "empty_signals"

echo "MATCH (n:Event {id: 'test-empty-signal'}) RETURN count(n) AS remaining;" | $MG
```

**Pass:** `empty_signals` > 0, `remaining = 0`.

## 6. Auto-Fix: Fake City-Center Coordinates

Seed a signal with coordinates at the city center (within 0.02 degrees) and verify they get nulled.

```bash
# Twin Cities center: 44.9778, -93.2650
echo "CREATE (n:Event {
  id: 'test-fake-coords',
  title: 'Test Fake Coords',
  summary: 'test',
  lat: 44.978,
  lng: -93.265
});" | $MG

cargo run --bin supervisor -- --city twincities 2>&1 | grep "fake_coords"

echo "MATCH (n:Event {id: 'test-fake-coords'}) RETURN n.lat, n.lng;" | $MG
```

**Pass:** `fake_coords` > 0, coordinates are null in verification.

**Cleanup:**

```bash
echo "MATCH (n:Event {id: 'test-fake-coords'}) DELETE n;" | $MG
```

## 7. Triage Heuristics

Verify each triage heuristic finds the right suspects.

### 7a. Misclassification Suspects

Low-confidence signals with single evidence source are flagged.

```bash
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
AND n.confidence < 0.5
OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
WITH n, count(ev) AS ev_count WHERE ev_count <= 1
RETURN labels(n)[0] AS type, n.title, n.confidence, ev_count
LIMIT 5;" | $MG
```

**Pass:** If this returns rows, the supervisor triage should find them as `Misclassification` suspects.

### 7b. Incoherent Story Suspects

Stories with high type diversity but few shared actors.

```bash
echo "MATCH (s:Story)-[:CONTAINS]->(sig)
WITH s, count(sig) AS cnt, count(DISTINCT labels(sig)[0]) AS types
WHERE types >= 3 AND cnt >= 3
RETURN s.headline, cnt, types;" | $MG
```

### 7c. Near-Duplicate Suspects

Signal pairs in the 0.85-0.92 similarity range.

```bash
echo "MATCH (a)-[r:SIMILAR_TO]-(b)
WHERE r.weight >= 0.85 AND r.weight < 0.92 AND a.id < b.id
RETURN a.title, b.title, r.weight LIMIT 5;" | $MG
```

### 7d. Low-Confidence High-Visibility

Low-confidence signals in confirmed stories.

```bash
echo "MATCH (s:Story {status: 'confirmed'})-[:CONTAINS]->(sig)
WHERE sig.confidence < 0.3
RETURN sig.title, sig.confidence, s.headline LIMIT 5;" | $MG
```

## 8. LLM Budget Cap

The supervisor limits LLM checks to 50 per run (DEFAULT_MAX_LLM_CHECKS).

```bash
cargo run --bin supervisor -- --city twincities 2>&1 | grep "LLM budget"
```

**Pass:** `llm_budget_used` <= 50 and `llm_budget_remaining` >= 0.

**Fail:** `llm_budget_used` > 50 — budget tracking in the LLM check phase is broken.

## 9. Issue Persistence and Deduplication

Run the supervisor twice and verify issues are not duplicated.

```bash
# Count issues before
echo "MATCH (vi:ValidationIssue {status: 'open'}) RETURN count(vi) AS issues;" | $MG

# Run supervisor
cargo run --bin supervisor -- --city twincities 2>&1 | grep "issues_created"

# Run again immediately
cargo run --bin supervisor -- --city twincities 2>&1 | grep "issues_created"
```

**Pass:** Second run shows `issues_created=0` (or very few) — same issues detected but deduped against open issues.

**Fail:** Issue count doubles — `create_if_new` dedup logic is broken.

Verify issue node structure:

```bash
echo "MATCH (vi:ValidationIssue)
RETURN vi.issue_type AS type, vi.severity AS severity, vi.status AS status,
       vi.target_label AS target, left(vi.description, 80) AS description
ORDER BY vi.created_at DESC LIMIT 10;" | $MG
```

## 10. Issue Expiry

Issues older than 30 days should be expired.

```bash
# Seed an old issue
echo "CREATE (vi:ValidationIssue {
  id: 'test-old-issue',
  city: 'twincities',
  issue_type: 'misclassification',
  severity: 'warning',
  target_id: 'fake-target',
  target_label: 'Event',
  description: 'test old issue',
  suggested_action: 'test',
  status: 'open',
  created_at: datetime() - duration('P60D')
});" | $MG

cargo run --bin supervisor -- --city twincities 2>&1 | grep -i "expire"

echo "MATCH (vi:ValidationIssue {id: 'test-old-issue'}) RETURN vi.status;" | $MG
```

**Pass:** Issue status is no longer `open` (either expired/resolved or deleted).

## 11. Source Penalty Feedback

Sources associated with open validation issues should receive quality penalties.

```bash
# Check sources with penalties applied
echo "MATCH (s:Source)
WHERE s.quality_penalty < 1.0
RETURN s.canonical_value AS source, s.quality_penalty AS penalty,
       s.source_type AS type
LIMIT 10;" | $MG
```

**Pass:** After supervisor runs with open issues, some sources have `quality_penalty < 1.0`.

Verify penalty reset when issues are resolved:

```bash
# Resolve all open issues for a source
echo "MATCH (vi:ValidationIssue {status: 'open'})
SET vi.status = 'resolved'
RETURN count(vi) AS resolved;" | $MG

# Run supervisor
cargo run --bin supervisor -- --city twincities 2>&1 | grep "sources_reset"
```

**Pass:** `sources_reset` > 0 — penalties cleared for sources with no remaining open issues.

## 12. Scout-Running Guard

The supervisor defers feedback writes when the scout is actively running.

```bash
# Simulate scout lock
echo "CREATE (l:ScoutLock {acquired_at: datetime()});" | $MG

cargo run --bin supervisor -- --city twincities 2>&1 | grep "deferring feedback"

# Cleanup
echo "MATCH (l:ScoutLock) DELETE l;" | $MG
```

**Pass:** Logs show `Scout is running, deferring feedback writes to next run`.

**Fail:** Source penalties applied while scout lock exists — `is_scout_running()` check is broken.

## 13. Echo Detection

Stories with high signal volume but low type and entity diversity should be flagged.

```bash
echo "MATCH (s:Story)
WHERE s.echo_score IS NOT NULL
RETURN s.headline, s.echo_score, s.signal_count, s.type_diversity, s.entity_count
ORDER BY s.echo_score DESC LIMIT 10;" | $MG
```

**Pass:**
- Stories with `echo_score > 0.7` have low type_diversity and/or low entity_count
- Stories with `echo_score < 0.3` have diverse types and entities
- Single-signal stories are not scored (or score = 0)

**Fail:**
- All echo_scores are null — `detect_echoes` is not running or not writing scores
- Diverse stories have high echo_score — `compute_echo_score` formula is inverted

## 14. Watermark Window

The supervisor uses a watermark to avoid re-checking old signals.

```bash
echo "MATCH (ss:SupervisorState)
RETURN ss.last_run_at AS last_run, ss.city AS city;" | $MG
```

**Pass:** `last_run_at` is updated to the end of the window after each run.

**Fail:** `last_run_at` is null or very old — `update_last_run` is broken, causing the supervisor to re-check everything on every run.

## 15. Full Supervisor + Scout Integration

Run a full scout then supervisor cycle and verify the graph is consistent.

```bash
# Scout run
docker compose run --rm scout 2>&1 | tee /tmp/scout.log

# Supervisor run
cargo run --bin supervisor -- --city twincities 2>&1 | tee /tmp/supervisor.log

# Validate
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

**All validation checks must pass after supervisor run.** The supervisor is non-destructive (auto-fixes remove only garbage data).

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| `Another supervisor is running` (no other process) | Stale SupervisorLock — delete manually |
| Auto-fix deletes real data | Heuristic too aggressive (e.g., coord epsilon too large) |
| Triage finds 0 suspects | Watermark window too narrow, or all signals above confidence thresholds |
| `issues_created` always 0 | LLM returning no issues for suspects, or budget exhausted before first check |
| Issues duplicate on re-run | `create_if_new` not matching on target_id + issue_type |
| Source penalties never applied | `apply_source_penalties` query not joining issues to sources |
| Source penalties never reset | `reset_resolved_penalties` not detecting resolved issues |
| Echo scores all null | `detect_echoes` query not matching stories with 5+ signals |
| Watermark not advancing | `update_last_run` failing silently |
| Notifications not sent | Slack webhook URL missing or `NotifyBackend` returning errors |
