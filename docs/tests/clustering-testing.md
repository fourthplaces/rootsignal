# Stories & Similarity Testing Playbook

Tests for the story pipeline (`story_weaver.rs`, `similarity.rs`, `story_metrics.rs`, `synthesizer.rs`). Verifies similarity edge construction, StoryWeaver materialization, story reconciliation, headline generation, velocity/energy computation, centroid/sensitivity propagation, signal type counts, gap scoring, and zombie archival.

**Architecture:** StoryWeaver is the sole story creator. Similarity edges exist for search only (no Leiden).

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
- `edges = 0` with 50+ embedded signals -- threshold too high or embeddings are garbage
- `min_weight` very close to 0 -- confidence weighting is broken

## 2. StoryWeaver Materialization

Stories are created from tension hubs with 2+ respondents. Verify stories anchor on tensions.

```bash
echo "MATCH (s:Story)-[:CONTAINS]->(t:Tension)
RETURN s.headline, t.title, s.signal_count
ORDER BY s.signal_count DESC LIMIT 10;" | $MG
```

**Pass:** Every story contains at least one Tension node.

```bash
echo "MATCH (s:Story) WHERE NOT (s)-[:CONTAINS]->(:Tension)
RETURN count(s) AS orphan_stories;" | $MG
```

**Must return 0.** Any orphan stories are Leiden artifacts that should have been cleaned up by migration.

## 3. Story Reconciliation (Containment Check)

Run the same city twice and verify existing stories evolve rather than duplicate.

```bash
# Count stories before
echo "MATCH (s:Story) RETURN count(s) AS stories;" | $MG

# Run again
cargo run --bin scout -- --city twincities 2>&1 | grep -iE "materialized|grown|absorbed|story weaving"

# Count stories after
echo "MATCH (s:Story) RETURN count(s) AS stories;" | $MG
```

**Pass:** Second run shows `grown` > 0 or `absorbed` > 0. Story count increases only by genuinely new tension hubs.

**Fail:** Story count roughly doubles -- containment threshold (0.5) is broken.

## 4. Story Status Classification

Stories should be classified as echo, confirmed, or emerging based on type diversity and entity count.

```bash
echo "MATCH (s:Story)
RETURN s.status AS status, count(*) AS count ORDER BY count DESC;" | $MG
```

**Pass:** All three statuses appear in a sufficiently diverse graph.

## 5. Story Headline Quality

Pull 5 random stories and evaluate their headlines.

```bash
echo "MATCH (s:Story)
RETURN s.headline, s.summary, s.signal_count, s.dominant_type
ORDER BY rand() LIMIT 5;" | $MG
```

**What to look for:**
- Headlines are specific and distinguish one story from another
- Headlines are under 80 characters
- Summaries add context beyond the headline

## 6. Story Synthesis

Stories should have lede, narrative, arc, category, and action_guidance after synthesis runs.

```bash
echo "MATCH (s:Story)
WHERE s.lede IS NOT NULL AND s.lede <> ''
RETURN s.headline, s.arc, s.category,
       left(s.lede, 100) AS lede_preview,
       left(s.narrative, 100) AS narrative_preview
LIMIT 5;" | $MG
```

**Pass:** Returns rows with meaningful lede and narrative text.

## 7. Velocity and Energy

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

## 8. Centroid Computation

Stories with geolocated signals should have centroid coordinates.

```bash
echo "MATCH (s:Story)
WHERE s.centroid_lat IS NOT NULL
RETURN s.headline, s.centroid_lat, s.centroid_lng, s.signal_count
LIMIT 5;" | $MG
```

**Pass:** Centroids are within the expected metro area, not at city center or at (0, 0).

## 9. Centroid Fuzzing for Sensitive Stories

Sensitive stories should have their centroids snapped to a grid (~5km for sensitive, ~500m for elevated).

```bash
echo "MATCH (s:Story)
WHERE s.sensitivity IN ['sensitive', 'elevated'] AND s.centroid_lat IS NOT NULL
RETURN s.headline, s.sensitivity, s.centroid_lat, s.centroid_lng;" | $MG
```

**Pass:** Coordinates look snapped (round numbers at expected precision).

## 10. Sensitivity Propagation

Story sensitivity should be the maximum of its constituent signals.

```bash
echo "MATCH (s:Story {sensitivity: 'sensitive'})-[:CONTAINS]->(sig)
WHERE sig.sensitivity = 'sensitive'
WITH s, count(sig) AS sensitive_count
RETURN s.headline, sensitive_count;" | $MG
```

**Pass:** Every sensitive story has at least one sensitive signal.

## 11. Signal Type Counts

Stories should have accurate ask_count, give_count, event_count.

```bash
echo "MATCH (s:Story)
WHERE s.ask_count > 0 OR s.give_count > 0 OR s.event_count > 0
RETURN s.headline, s.ask_count, s.give_count, s.event_count, s.drawn_to_count, s.gap_score
ORDER BY s.gap_score DESC LIMIT 10;" | $MG
```

**Pass:** Counts match what's in the CONTAINS set. `gap_score = ask_count - give_count`.

## 12. Gap Score Verification

```bash
echo "MATCH (s:Story)
WHERE s.gap_score > 0
RETURN s.headline, s.ask_count, s.give_count, s.gap_score, s.gap_velocity
ORDER BY s.gap_score DESC LIMIT 5;" | $MG
```

**Pass:** Stories with positive gap_score have more asks than gives -- unmet community needs.

## 13. Unresponded Tensions

```bash
echo "MATCH (t:Tension)
WHERE NOT (t)<-[:CONTAINS]-(:Story)
OPTIONAL MATCH (t)<-[:RESPONDS_TO|DRAWN_TO]-(r)
WITH t, count(r) AS resp_count
WHERE resp_count < 2
RETURN t.title, resp_count, t.cause_heat
ORDER BY t.cause_heat DESC LIMIT 10;" | $MG
```

**Pass:** Returns tensions that haven't yet accumulated enough respondents for a story.

## 14. Zombie Story Archival

```bash
echo "MATCH (s:Story {arc: 'archived'})
RETURN s.headline, s.last_updated, s.velocity
LIMIT 5;" | $MG
```

**Pass:** Archived stories have `last_updated` > 30 days ago and velocity <= 0.

## 15. Pipeline Unchanged

After story weaving, all existing scout validation checks must still pass.

```bash
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| 0 similarity edges | All embeddings null, or cosine threshold too high |
| Stories without Tension | Leiden artifacts not cleaned up by migration |
| Stories duplicate on re-run | Containment threshold wrong, or `get_existing_stories` query broken |
| All stories "emerging" | Entity count or type_diversity not being computed from signal metadata |
| All velocities 0 | `ClusterSnapshot` not being written, or `get_snapshot_entity_count_7d_ago` returning None |
| Synthesis missing | Anthropic API error, or Phase C query not finding unsynthesized stories |
| Centroids at (0,0) | lat/lng zero-filtering broken in fetch_signal_metadata |
| gap_score always 0 | ask_count/give_count not being computed in phase_materialize or refresh_story_metadata |
| No archived stories after 30+ days | Phase D zombie detection not running, or last_updated not being set |
