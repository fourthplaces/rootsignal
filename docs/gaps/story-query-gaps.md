---
date: 2026-02-19
category: queries
source: story-pipeline-consolidation-brainstorm
---

# Story Query Gaps

Gaps identified during story pipeline consolidation pressure testing. These are queries real users would ask that the system can't currently answer.

## Gap 1: Unresponded Tensions

**User question:** "What needs exist that nobody is helping with yet?"

**Who needs this:** Donors, volunteers, funders, journalists

**Problem:** A tension with 0-1 respondents doesn't meet the 2+ threshold to become a story. These tensions are invisible to the story layer and the search app. But unresponded tensions are exactly what donors and volunteers most want to find — the gaps where no one is helping yet. This is the alignment machine's most powerful signal: where misalignment is forming.

**What exists today:** Tension nodes exist in the graph with `cause_heat`, but no API exposes "tensions that aren't yet stories."

**Fix:** Add `unresponded_tensions_in_bounds(bbox, limit)` to reader.rs and a matching GraphQL query.

```cypher
MATCH (t:Tension)
WHERE t.lat >= $min_lat AND t.lat <= $max_lat
  AND t.lng >= $min_lng AND t.lng <= $max_lng
  AND NOT (t)<-[:CONTAINS]-(:Story)
OPTIONAL MATCH (t)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
WITH t, count(sig) AS respondent_count
WHERE respondent_count < 2
RETURN t, respondent_count
ORDER BY t.cause_heat DESC
LIMIT $limit
```

**Effort:** Small. One reader method, one GraphQL query, no schema changes.

**Addressable in consolidation work:** Yes. We're already touching reader.rs and schema.rs.

---

## Gap 2: Time-Filtered Signals Within a Story

**User question:** "What can I do THIS WEEK about this story?"

**Who needs this:** Volunteers, attendees, organizers — anyone looking for action items

**Problem:** `story.signals` returns all constituent signals including past events and resolved asks. A user drilling into a story wants to see *current* action items: upcoming events, active asks, live gives.

**What exists today:** Signals have `starts_at`/`ends_at` (Events), `extracted_at`, and `last_confirmed_active`. The graph supports time filtering. But the `story.signals` resolver and `get_story_signals` reader method don't accept time parameters.

**Fix:** Add optional time filter to the story signals query.

```cypher
// Upcoming events within a story
MATCH (s:Story {id: $id})-[:CONTAINS]->(e:Event)
WHERE e.starts_at >= datetime()
RETURN e ORDER BY e.starts_at

// Active asks within a story
MATCH (s:Story {id: $id})-[:CONTAINS]->(a:Ask)
WHERE a.last_confirmed_active >= datetime() - duration('P7D')
RETURN a ORDER BY a.extracted_at DESC
```

**Effort:** Small. Add optional `upcoming` or `since` parameter to existing resolver.

**Addressable in consolidation work:** Yes. We're already touching story_weaver.rs and the story API.

---

## Gap 3: Actors in a Story

**User question:** "Who else is working on this problem?"

**Who needs this:** Nonprofit directors (collaboration), funders (landscape), journalists (sourcing)

**Problem:** Actor nodes have ACTED_IN edges to signals, and signals are CONTAINS'd by stories. The graph path exists: `Story → CONTAINS → Signal ← ACTED_IN ← Actor`. But there's no `story.actors` resolver — no way to see which organizations appear in a story.

**What exists today:** The path is traversable in Cypher. No API exposes it.

**Fix:** Add `actors` resolver to `GqlStory`.

```cypher
MATCH (s:Story {id: $id})-[:CONTAINS]->(sig)<-[:ACTED_IN]-(a:Actor)
RETURN DISTINCT a, count(sig) AS signal_count
ORDER BY signal_count DESC
```

**Effort:** Small. One reader method, one resolver on GqlStory. Actor types already exist.

**Addressable in consolidation work:** Yes, if we're already modifying GqlStory to add the new sorting metrics.

---

## Gap 4: Gap Trajectory Over Time

**User question:** "Is this getting better or worse?"

**Who needs this:** Funders tracking impact, journalists tracking trends, organizers deciding where to focus

**Problem:** We're adding gap_score (Asks minus Gives) as a story metric. But gap_score is a point-in-time snapshot. A donor wants to know: is the gap widening (more asks appearing) or closing (more gives arriving)? That's the alignment machine's temporal dimension.

**What exists today:** ClusterSnapshot tracks `signal_count` and `entity_count` over time. Velocity is computed from entity_count deltas. But there's no ask_count/give_count history — so gap trajectory can't be computed.

**Fix:** Expand ClusterSnapshot to include `ask_count` and `give_count`. Phase D already creates snapshots — just add two fields. Then compute "gap velocity" alongside regular velocity.

```rust
pub struct ClusterSnapshot {
    pub id: Uuid,
    pub story_id: Uuid,
    pub signal_count: u32,
    pub entity_count: u32,
    pub ask_count: u32,    // NEW
    pub give_count: u32,   // NEW
    pub run_at: DateTime<Utc>,
}

// Gap velocity = (current_gap - gap_7d_ago) / 7
// Positive = gap widening (bad), Negative = gap closing (good)
```

**Effort:** Moderate. Expand snapshot struct, expand Phase D computation, add `gap_velocity` to story metrics. Touching this code anyway during consolidation.

**Addressable in consolidation work:** Yes. We're moving `compute_velocity_and_energy()` into StoryWeaver Phase D and already expanding what's computed there.

---

## Gap 5: Story-to-Story Relationships

**User question:** "How are these stories related?"

**Who needs this:** Journalists seeing connections, researchers studying systemic issues

**Problem:** "Housing affordability" and "Gentrification displacement" are separate stories. They're clearly related (shared signals, related tensions) but no explicit connection exists. No RELATED_TO edge between stories.

**What exists today:** The only connection is shared signals (detected by containment check during materialization) or shared actors (via multi-hop traversal). Neither is exposed.

**Fix options:**
- Compute story-story similarity from signal overlap percentage (cheap, during Phase D)
- Compute from tension embedding similarity (uses existing embeddings)
- Create explicit RELATED_TO edges between stories that share > N% signals

**Effort:** Moderate to large. Requires new edge type, computation logic, and API exposure.

**Addressable in consolidation work:** No. Follow-up. Users can read both stories and make the connection themselves for now.

---

## Gap 6: Actor-Centric Story View

**User question:** "Show me all stories this organization appears in — are they working across issues?"

**Who needs this:** Funders evaluating systemic impact, journalists profiling organizations

**Problem:** No actor profile view showing which stories they appear in. Requires: `Actor → ACTED_IN → Signal → CONTAINS ← Story`. The traversal works but isn't exposed as an API.

**What exists today:** Actor nodes exist. The graph path works. No dedicated query or API.

**Fix:** Add `stories_by_actor(actor_id)` to reader.rs.

```cypher
MATCH (a:Actor {id: $id})-[:ACTED_IN]->(sig)<-[:CONTAINS]-(s:Story)
RETURN DISTINCT s, count(sig) AS involvement
ORDER BY involvement DESC
```

**Effort:** Small. One reader method, one GraphQL query.

**Addressable in consolidation work:** No. Follow-up. Not related to the pipeline changes.

---

## Summary

| Gap | User Need | Effort | In Scope? |
|-----|-----------|--------|-----------|
| 1. Unresponded tensions | "Where is no one helping?" | Small | **Yes** |
| 2. Time-filtered story signals | "What can I do this week?" | Small | **Yes** |
| 3. Actors in a story | "Who's working on this?" | Small | **Yes** |
| 4. Gap trajectory | "Is this getting better or worse?" | Moderate | **Yes** |
| 5. Story relationships | "How are these stories related?" | Large | No — follow-up |
| 6. Actor-centric view | "What's this org involved in?" | Small | No — follow-up |

Gaps 1-4 are addressable during the story pipeline consolidation because we're already touching the relevant code (reader.rs, story_weaver.rs, GraphQL schema, ClusterSnapshot, Phase D computation).
