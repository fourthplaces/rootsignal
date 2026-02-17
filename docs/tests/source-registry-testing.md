# Source Registry Testing Playbook

Tests for the Source Node Foundation (Phase 1 of Emergent Source Discovery).

## Prerequisites

```bash
docker compose up -d memgraph
MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"
```

## 1. Source Seeding — All Cities

Run each city and verify sources are seeded correctly.

```bash
for city in twincities nyc portland berlin; do
  echo "=== $city ==="
  docker compose run --rm -e CITY=$city scout 2>&1 | grep -i "seed\|source reg"
  echo ""
done
```

**Expected:**

| City | Seeded | Source Registry (total/active/curated/discovered) |
|------|--------|--------------------------------------------------|
| twincities | 85 | ~84 / ~84 / ~84 / 0 |
| nyc | 9 | 9 / 9 / 9 / 0 |
| portland | 9 | 9 / 9 / 9 / 0 |
| berlin | 7 | 7 / 7 / 7 / 0 |

Twin Cities shows 84 instead of 85 because one URL deduplicates on MERGE.

**Fail criteria:**
- `seeded=0` — upsert_source is broken
- `Source registry stats` line missing — get_source_stats query failing
- discovered > 0 when no discovery phase exists yet — something is mislabeling sources

## 2. Source Seeding Idempotency

Run the same city twice. Source count must not double.

```bash
docker compose run --rm scout 2>&1 | grep "source reg"
docker compose run --rm scout 2>&1 | grep "source reg"
```

**Pass:** Both runs show the same `total` count.

**Fail:** Second run shows higher `total` — MERGE on url is broken, creating duplicates.

Verify in the graph:

```bash
echo "MATCH (s:Source) WITH s.url AS url, count(*) AS c WHERE c > 1 RETURN url, c;" | $MG
```

Must return empty. Any rows mean the url uniqueness constraint or MERGE logic is broken.

## 3. Multi-City Isolation

After running multiple cities, verify sources are scoped correctly.

```bash
echo "MATCH (s:Source) RETURN s.city AS city, count(*) AS sources ORDER BY sources DESC;" | $MG
```

**Pass:** Each city has its own count. No cross-contamination.

**Fail:** A city shows sources from another city's profile.

Also verify that `get_active_sources` respects city scoping:

```bash
echo "MATCH (s:Source {city: 'New York City'}) WHERE s.url CONTAINS 'minneapolis' RETURN count(s) AS wrong;" | $MG
```

Must return 0.

## 4. Source Node Property Completeness

Every Source node must have all required properties set.

```bash
echo "MATCH (s:Source)
WHERE s.id IS NULL OR s.url IS NULL OR s.source_type IS NULL
   OR s.discovery_method IS NULL OR s.city IS NULL OR s.trust IS NULL
   OR s.initial_trust IS NULL OR s.active IS NULL
RETURN s.url, s.id IS NULL AS missing_id, s.trust IS NULL AS missing_trust;" | $MG
```

Must return empty. Any rows mean upsert_source is not setting all properties.

## 5. Source Type Distribution

Verify sources have correct types based on their URLs.

```bash
echo "MATCH (s:Source) RETURN s.source_type AS type, s.discovery_method AS method, count(*) AS count ORDER BY count DESC;" | $MG
```

**Expected for Twin Cities:**
- web / curated: ~45
- instagram / curated: ~25
- facebook / curated: ~11
- reddit / curated: ~3

**Fail criteria:**
- All sources typed as "web" — source_type assignment in seed_curated_sources is broken
- Any source with discovery_method other than "curated" — mislabeling

## 6. Trust Score Correctness

Trust scores should match the TLD-based heuristic from `source_trust()`.

```bash
# .gov sources should have trust 0.9
echo "MATCH (s:Source) WHERE s.url CONTAINS '.gov' RETURN s.url, s.trust LIMIT 5;" | $MG

# .org sources should have trust 0.8
echo "MATCH (s:Source) WHERE s.url CONTAINS '.org' AND NOT s.url CONTAINS '.gov' RETURN s.url, s.trust LIMIT 5;" | $MG

# Instagram sources should have trust 0.3
echo "MATCH (s:Source) WHERE s.source_type = 'instagram' RETURN s.url, s.trust LIMIT 5;" | $MG
```

**Pass:** Trust values match the heuristic in `sources.rs:source_trust()`.

**Fail:** All trusts are the same value, or trust doesn't match expected TLD-based score.

Also verify trust = initial_trust (no drift before any trust adjustment logic exists):

```bash
echo "MATCH (s:Source) WHERE s.trust <> s.initial_trust RETURN s.url, s.trust, s.initial_trust;" | $MG
```

Must return empty.

## 7. Dead Source Deactivation

Curated sources should never be deactivated. Only non-curated sources with 10+ consecutive empty runs.

```bash
# No curated sources should be inactive
echo "MATCH (s:Source {discovery_method: 'curated', active: false}) RETURN count(s) AS deactivated_curated;" | $MG
```

Must return 0.

To test the deactivation logic itself, manually set a non-curated source's consecutive_empty_runs high and re-run:

```bash
# Create a fake discovered source with 10 empty runs
echo "CREATE (s:Source {
  id: 'test-dead-source',
  url: 'https://example.com/dead',
  source_type: 'web',
  discovery_method: 'gap_analysis',
  city: 'Twin Cities (Minneapolis-St. Paul, Minnesota)',
  trust: 0.5,
  initial_trust: 0.5,
  created_at: datetime(),
  signals_produced: 0,
  signals_corroborated: 0,
  consecutive_empty_runs: 10,
  active: true,
  gap_context: 'test'
});" | $MG

# Run scout
docker compose run --rm scout 2>&1 | grep -i "deactivat"

# Verify it was deactivated
echo "MATCH (s:Source {url: 'https://example.com/dead'}) RETURN s.active;" | $MG
# Should return false

# Cleanup
echo "MATCH (s:Source {url: 'https://example.com/dead'}) DELETE s;" | $MG
```

## 8. Blocked Source Check

Test that the blocked source mechanism works.

```bash
# Create a blocked source
echo "CREATE (b:BlockedSource {url_pattern: 'example.com/spam', blocked_at: datetime(), reason: 'test'});" | $MG

# Verify it exists
echo "MATCH (b:BlockedSource) RETURN b.url_pattern, b.reason;" | $MG

# Cleanup
echo "MATCH (b:BlockedSource {url_pattern: 'example.com/spam'}) DELETE b;" | $MG
```

This is a schema-only test for now. The `is_blocked()` check will be exercised when the discovery phase (Phase 2/3) creates new sources.

## 9. Pipeline Behavior Unchanged

After source seeding, the core pipeline must behave identically to before.

```bash
docker compose run --rm scout 2>&1 | tee /tmp/scout-source-test.log
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

**All 8 existing validation checks must still pass.** Source nodes are additive — they should not affect scraping, extraction, dedup, or storage.

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| `seeded=0` | Memgraph connection issue or MERGE syntax error |
| Source count doubles on re-run | MERGE not matching on url, or url uniqueness constraint missing |
| Sources from wrong city appear | city field not being set or get_active_sources not filtering |
| All trust scores identical | source_trust() not being called per-URL during seeding |
| Curated sources deactivated | deactivate_dead_sources not excluding discovery_method='curated' |
| "Source registry stats" missing from logs | get_source_stats query failing silently |
| Existing validation checks fail | Source seeding accidentally modified pipeline behavior |
