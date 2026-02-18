# Clustering & Stories Testing Playbook

Tests for the clustering pipeline (`cluster.rs`, `similarity.rs`, `synthesizer.rs`). Verifies similarity edge construction, Leiden community detection, story reconciliation, headline generation, velocity/energy computation, and the mega-cluster guard.

## Prerequisites

```bash
docker compose up -d memgraph web
MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"
```

The graph must have at least 50 signals with embeddings from a prior scout run.

## 1. Similarity Edges Created

After a scout run, verify SIMILAR_TO edges exist with correct properties.

```bash
cargo run --bin scout -- --city twincities 2>&1 | grep -iE "similarity|SIMILAR_TO|edges"
```

**Expected output:**
- `Fetched signal embeddings for similarity computation signals=N` (N should match total embedded signals)
- `Computed similarity edges above threshold 0.65 edges=N`
- `SIMILAR_TO edges written total_created=N`

Verify in the graph:

```bash
echo "MATCH ()-[r:SIMILAR_TO]->()
RETURN count(r) AS edges, round(avg(r.weight) * 100) / 100 AS avg_weight,
       round(min(r.weight) * 100) / 100 AS min_weight,
       round(max(r.weight) * 100) / 100 AS max_weight;" | $MG
```

**Pass:**
- `edges` > 0
- `min_weight` > 0 (weight = cosine * geometric_mean(conf_a, conf_b))
- `avg_weight` between 0.3 and 0.9 (reasonable range)

**Fail:**
- `edges = 0` with 50+ embedded signals — threshold too high or embeddings are garbage
- `min_weight` very close to 0 — confidence weighting is broken

## 2. Similarity Threshold Regression (0.65)

The threshold was raised from 0.55 to 0.65 in commit `669a6c0` to fix mega-clusters. Verify the threshold is effective.

```bash
# Check the weight distribution — no edges should have raw cosine below 0.65
# (weight = cosine * conf_weight, so weight can be lower than 0.65, but the
# underlying cosine must be >= 0.65)
echo "MATCH (a)-[r:SIMILAR_TO]->(b)
WITH r.weight AS w, a.confidence AS ca, b.confidence AS cb
WITH w, CASE WHEN ca * cb > 0 THEN w / sqrt(ca * cb) ELSE 0 END AS approx_cosine
WHERE approx_cosine < 0.64
RETURN count(*) AS below_threshold;" | $MG
```

**Pass:** `below_threshold = 0`.

**Fail:** Edges exist below the cosine threshold — the similarity builder is using the wrong constant.

## 3. Minimum Connected Signals Gate

Clustering requires at least 10 connected signals (signals with at least one SIMILAR_TO edge). On a sparse graph, clustering should be skipped.

```bash
echo "MATCH (n)-[:SIMILAR_TO]-()
WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
RETURN count(DISTINCT n) AS connected;" | $MG
```

**Pass:** If `connected < 10`, the scout log should show `Insufficient connected signals for clustering` and no stories are created.

**Fail:** Stories created from fewer than 10 connected signals — the `MIN_CONNECTED_SIGNALS` gate is broken.

## 4. Leiden Community Detection

Verify communities are detected and have reasonable sizes.

```bash
echo "MATCH (s:Story)-[:CONTAINS]->(sig)
RETURN s.id AS story_id, s.headline AS headline, count(sig) AS signals
ORDER BY signals DESC LIMIT 15;" | $MG
```

**Pass:**
- Multiple stories exist (not just one mega-story)
- Signal counts per story range from 2 to ~30
- No story has more than 30 signals (MAX_COMMUNITY_SIZE guard)

**Fail:**
- Only 1 story containing most signals — mega-cluster, Leiden gamma too low
- Many single-signal stories — gamma too high or threshold too low

## 5. Mega-Cluster Guard

No community should exceed 30 signals. This is the safety net for when Leiden produces oversized clusters.

```bash
echo "MATCH (s:Story)-[:CONTAINS]->(sig)
WITH s, count(sig) AS cnt
WHERE cnt > 30
RETURN s.headline, cnt;" | $MG
```

**Must return empty.** If any story exceeds 30 signals, the `MAX_COMMUNITY_SIZE` filter in `run_leiden()` is not working.

## 6. Story Reconciliation (Asymmetric Containment)

Run the same city twice and verify existing stories evolve rather than duplicate.

```bash
# Count stories before
echo "MATCH (s:Story) RETURN count(s) AS stories;" | $MG

# Run again
cargo run --bin scout -- --city twincities 2>&1 | grep -iE "stories created|stories updated|clustering"

# Count stories after
echo "MATCH (s:Story) RETURN count(s) AS stories;" | $MG
```

**Pass:** Second run shows `stories_updated` > 0. Story count increases only by genuinely new clusters, not by duplicating existing ones.

**Fail:** Story count roughly doubles — reconciliation via asymmetric containment (threshold 0.5) is broken.

Verify story identity is preserved:

```bash
echo "MATCH (s:Story)
WHERE s.first_seen < datetime() - duration('PT1H')
  AND s.last_updated > datetime() - duration('PT1H')
RETURN s.headline, s.first_seen, s.last_updated, s.signal_count
LIMIT 10;" | $MG
```

**Pass:** Stories have `first_seen` from the original run but `last_updated` from the latest run — identity preserved, metadata updated.

## 7. Story Status Classification

Stories should be classified as echo, confirmed, or emerging based on type diversity and entity count.

```bash
echo "MATCH (s:Story)
RETURN s.status AS status, count(*) AS count ORDER BY count DESC;" | $MG
```

**Pass:** All three statuses appear in a sufficiently diverse graph.

Verify the classification rules:

```bash
# Echo: single type, 5+ signals
echo "MATCH (s:Story {status: 'echo'})-[:CONTAINS]->(sig)
WITH s, count(sig) AS cnt, count(DISTINCT labels(sig)[0]) AS types
RETURN s.headline, cnt, types;" | $MG
# All rows should have types = 1 and cnt >= 5

# Confirmed: 2+ entities AND 2+ types
echo "MATCH (s:Story {status: 'confirmed'})
RETURN s.headline, s.entity_count, s.type_diversity;" | $MG
# All rows should have entity_count >= 2 AND type_diversity >= 2

# Emerging: everything else
echo "MATCH (s:Story {status: 'emerging'})
RETURN s.headline, s.entity_count, s.type_diversity, s.signal_count;" | $MG
```

**Fail:** A story classified as `confirmed` has `type_diversity = 1` or `entity_count = 1` — `story_status()` logic is wrong.

## 8. Story Headline Quality

Pull 5 random stories and evaluate their headlines.

```bash
echo "MATCH (s:Story)
RETURN s.headline, s.summary, s.signal_count, s.dominant_type
ORDER BY rand() LIMIT 5;" | $MG
```

**What to look for:**
- Headlines are specific and distinguish one story from another
- Headlines are under 80 characters
- No generic category labels ("Community events", "Housing issues")
- Summaries add context beyond the headline

**What is NOT a problem:**
- Fallback headlines using first signal title — that's the LLM failure fallback
- Summaries being generic ("Cluster of related civic signals") — means LLM call failed but pipeline continued

## 9. Story Synthesis

Stories should have lede, narrative, arc, category, and action_guidance after synthesis runs.

```bash
echo "MATCH (s:Story)
WHERE s.lede IS NOT NULL AND s.lede <> ''
RETURN s.headline, s.arc, s.category,
       left(s.lede, 100) AS lede_preview,
       left(s.narrative, 100) AS narrative_preview
LIMIT 5;" | $MG
```

**Pass:** Returns rows with meaningful lede and narrative text. Arc is one of: Emerging, Growing, Stable, Fading.

Check unsynthesized stories:

```bash
echo "MATCH (s:Story)
WHERE s.lede IS NULL OR s.lede = ''
RETURN count(s) AS unsynthesized;" | $MG
```

After a full run, this should be 0 (all stories get synthesized). If > 0, check logs for `Story synthesis LLM call failed`.

## 10. Velocity and Energy

Velocity tracks entity diversity growth over 7 days. Energy combines velocity, recency, source diversity, and triangulation.

```bash
echo "MATCH (s:Story)
RETURN s.headline, s.velocity, s.energy, s.entity_count, s.type_diversity,
       s.source_count, s.signal_count
ORDER BY s.energy DESC LIMIT 10;" | $MG
```

**Pass:**
- Stories with high type_diversity and entity_count have higher energy
- Velocity is positive for growing stories, near-zero for stable ones
- Energy values are between 0 and ~2 (can exceed 1.0 with high velocity)

**Fail:**
- All velocities are 0 — `compute_velocity_and_energy` or snapshot creation is broken
- All energies are identical — formula components are constant

Verify snapshots exist:

```bash
echo "MATCH (cs:ClusterSnapshot)
RETURN count(cs) AS snapshots,
       min(cs.run_at) AS earliest,
       max(cs.run_at) AS latest;" | $MG
```

**Pass:** Snapshots exist with timestamps matching scout runs.

## 11. Energy Formula Weights

Confirmed, well-triangulated stories should structurally outrank single-type echo clusters at equal velocity.

```bash
echo "MATCH (s:Story)
WHERE s.energy IS NOT NULL
WITH s,
     CASE WHEN s.type_diversity >= 3 THEN 'triangulated'
          WHEN s.type_diversity = 1 AND s.signal_count >= 5 THEN 'echo'
          ELSE 'other' END AS category
RETURN category, round(avg(s.energy) * 100) / 100 AS avg_energy,
       count(s) AS count
ORDER BY avg_energy DESC;" | $MG
```

**Pass:** `triangulated` stories have higher average energy than `echo` stories (triangulation contributes 25% of energy vs echo's 5%).

## 12. Centroid Computation

Stories with geolocated signals should have centroid coordinates.

```bash
echo "MATCH (s:Story)
WHERE s.centroid_lat IS NOT NULL
RETURN s.headline, s.centroid_lat, s.centroid_lng, s.signal_count
LIMIT 5;" | $MG
```

**Pass:** Centroids are within the expected metro area, not at city center or at (0, 0).

```bash
# Stories without centroids should have no geolocated signals
echo "MATCH (s:Story)
WHERE s.centroid_lat IS NULL
OPTIONAL MATCH (s)-[:CONTAINS]->(sig)
WHERE sig.lat IS NOT NULL
WITH s, count(sig) AS geolocated
WHERE geolocated > 0
RETURN s.headline, geolocated;" | $MG
```

**Must return empty.** Any rows mean centroid computation is skipping valid coordinates.

## 13. Sensitivity Propagation

Story sensitivity should be the maximum of its constituent signals.

```bash
echo "MATCH (s:Story {sensitivity: 'sensitive'})-[:CONTAINS]->(sig)
WHERE sig.sensitivity = 'sensitive'
WITH s, count(sig) AS sensitive_count
RETURN s.headline, sensitive_count;" | $MG
```

**Pass:** Every sensitive story has at least one sensitive signal.

## 14. Pipeline Unchanged

After clustering, all existing scout validation checks must still pass.

```bash
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| 0 similarity edges | All embeddings null, or cosine threshold too high |
| One mega-story with all signals | Similarity threshold too low (should be 0.65), or Leiden gamma too low |
| Stories duplicate on re-run | Asymmetric containment threshold wrong, or `get_existing_stories` query broken |
| All stories "emerging" | Entity count or type_diversity not being computed from signal metadata |
| All velocities 0 | `ClusterSnapshot` not being written, or `get_snapshot_entity_count_7d_ago` returning None |
| Headlines generic | Haiku structured output regression, or signal metadata not being passed to `label_cluster` |
| Synthesis missing | Anthropic API error, or `synthesize_stories` query not finding unsynthesized stories |
| GDS error on Leiden | Neo4j GDS plugin not installed, or graph projection syntax changed |
| `Skipping mega-cluster` warnings | Leiden gamma needs tuning (increase beyond 1.5 for finer clusters) |
