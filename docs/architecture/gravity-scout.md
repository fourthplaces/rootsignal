# Gravity Scout: Where Are People Gathering Around This Tension?

The Gravity Scout is the third investigation mode in the scout pipeline. Where the curiosity loop asks *"why does this signal exist?"* and discovers **tensions**, and the Response Scout asks *"what diffuses this tension?"* and discovers **instrumental responses**, the Gravity Scout asks *"where are people gathering around this tension?"* and discovers **solidarity, community formation, and cultural crystallization**.

## The Three Investigation Modes

| Mode | Question | Discovers |
|------|----------|-----------|
| Curiosity Loop | "Why does this signal exist?" | Tensions |
| Response Scout | "What diffuses this tension?" | Instrumental responses (legal aid, mutual aid, boycotts) |
| Gravity Scout | "Where are people gathering around this tension?" | Solidarity, community formation, cultural crystallization |

## Problem

The Response Scout finds things that *solve* problems. But some of the most important community signals aren't about solving — they're about coming together. The singing rebellion in the Twin Cities is the canonical example: people gathering at churches and in the streets to sing prayer and song together. ICE enforcement fear doesn't get "solved" by singing — but the singing IS the community's response to the pressure. Tension as a force that brings out the best in people.

These gatherings are energizing, connective, and often the most important signals in the graph — but the existing pipeline doesn't search for them because they're not instrumental responses.

## Design Philosophy

### Tension creates gravity

High-heat tensions pull people together. The gravity scout asks: where is that gravitational pull manifesting? What gatherings, movements, cultural moments are forming in the tension's field?

### Not solving — gathering

A "Know Your Rights Workshop" solves a problem. A "Singing Rebellion at Lake Street Church" doesn't solve ICE enforcement — it transforms fear into solidarity. Both matter. The response scout finds the first. The gravity scout finds the second.

### Same node types, different discovery

A vigil is an Event. A solidarity fund is a Give. A call to gather is an Ask. The gravity scout reuses existing node types. What's different is the investigation prompt, the search strategy, and a `gathering_type` property on the RESPONDS_TO edge to distinguish gravitational pull from instrumental response.

### Follow the energy

The prompt encourages the LLM to look for where people are showing up physically, emotionally, creatively. Churches, streets, parks, community centers, online spaces where solidarity forms. Not organizations with programs — gatherings with people.

## Architecture

### Two-phase pattern (mirrors curiosity loop and response scout)

1. **Agentic investigation** — multi-turn conversation with `web_search` + `read_page` tools. Up to 10 tool turns to allow following threads.

2. **Structured extraction** — single `extract()` call to get structured `GravityFinding` JSON containing discovered gatherings, future query seeds, and an early-termination flag.

### Target selection

```cypher
MATCH (t:Tension)
WHERE t.confidence >= 0.5
  AND coalesce(t.cause_heat, 0.0) >= 0.1
  AND coalesce(datetime(t.gravity_scouted_at), datetime('2000-01-01'))
      < datetime() - duration({days:
          CASE
            WHEN coalesce(t.gravity_scout_miss_count, 0) = 0 THEN 7
            WHEN coalesce(t.gravity_scout_miss_count, 0) = 1 THEN 14
            WHEN coalesce(t.gravity_scout_miss_count, 0) = 2 THEN 21
            ELSE 30
          END
        })
ORDER BY t.cause_heat DESC, t.confidence DESC
LIMIT $limit
```

Key differences from response scout:

- **Requires `cause_heat >= 0.1`** — gravity only forms around active tensions. Cold tensions don't pull people together. The response scout has no heat minimum because instrumental responses can exist for dormant tensions.
- **Sorted by `cause_heat DESC`** — hottest tensions first. The response scout sorts by `response_count ASC` (most neglected first). Gravity reverses this: the hottest tensions are the most likely to create gatherings.
- **Exponential backoff** — 7 days after success or first attempt, scaling to 30 days max after 3+ consecutive misses. The response scout uses a fixed 14-day window. Gravity needs adaptive timing because most tensions don't create visible gatherings.
- **Fewer targets per run** — 3 vs the response scout's 5. Gravity is rarer than instrumental response; investigating fewer hot tensions deeply is better than spreading thin.

### Embedded triage (not a separate call)

Rather than a separate pre-filter call, triage is embedded in the investigation itself. The investigation prompt tells the LLM: "After 2-3 initial searches, if you find no evidence of gatherings, stop early and report `no_gravity: true`."

This is more durable than a separate triage call because:
- The LLM decides based on *actual search results*, not just the tension description
- Unexpected gatherings get discovered (housing crisis → tenant potlucks)
- Early termination saves budget (2-3 Tavily calls when there's nothing, vs 10 for a full run)
- No false negatives from a prediction-only filter

Same pattern as the curiosity loop's `curious: true/false`.

### Gathering types the LLM looks for

The prompt covers a wide taxonomy of gathering types, all freeform:

- **Solidarity & identity**: singing events, vigils, marches, cultural events
- **Environmental & safety**: community cleanups, neighborhood watches, town halls
- **Economic & housing**: tenant meetups, swap meets, solidarity potlucks
- **Health & wellbeing**: support circles, healing spaces, peer support
- **Civic & democratic**: packed school board meetings, petition drives, citizen journalism
- **Digital**: Instagram organizing accounts, Facebook groups, GoFundMe campaigns, Nextdoor threads

### Finding processing

For each `DiscoveredGathering`:
1. **Embed** title+summary via TextEmbedder
2. **Dedup** via `writer.find_duplicate(embedding, node_type, 0.85)`
   - If match: reuse existing signal, **touch timestamp** to prevent aging out, still create gravity edge
   - If new: create signal node directly
3. **Create node** — Event (with `organizer` and `is_recurring` populated), Give, or Ask. Confidence 0.7, city-center geo.
4. **Wire gravity edge** with `gathering_type` property on RESPONDS_TO
5. **Wire `also_addresses`** — same embedding-based multi-tension wiring as response scout, but calls `create_gravity_edge` (not `create_response_edge`) so all edges carry the `gathering_type`
6. **Venue seeding** — for each gathering with a `venue`, create a future source: `"{venue} {city} community events"`. Venues are gravitational centers that likely host more events than the one discovered.

For `future_queries`:
- Create TavilyQuery sources with `SourceRole::Response` and gravity-specific gap context

### Edge wiring: `create_gravity_edge`

```cypher
MATCH (resp) WHERE resp.id = $resp_id AND (resp:Give OR resp:Event OR resp:Ask)
MATCH (t:Tension {id: $tension_id})
MERGE (resp)-[r:RESPONDS_TO]->(t)
ON CREATE SET
    r.match_strength = $strength,
    r.explanation = $explanation,
    r.gathering_type = $gathering_type
ON MATCH SET
    r.gathering_type = coalesce(r.gathering_type, $gathering_type)
```

Why `ON CREATE`/`ON MATCH`? If the Response Scout already wired this signal to this tension as an instrumental response, a `MERGE` with properties in the pattern would fail to match (different properties) and create a duplicate edge. Merging on structure only, then setting properties conditionally, avoids the multi-graph trap. `ON MATCH` uses `coalesce` so an existing instrumental response doesn't get its `gathering_type` overwritten — but if it had none, we add one.

The `gathering_type` property on RESPONDS_TO distinguishes instrumental responses (no `gathering_type`) from gravitational gatherings (`gathering_type = "vigil"`, `"singing"`, `"solidarity meal"`, etc.). Downstream code can filter by `gathering_type IS NOT NULL` to find gravity edges.

### Exponential backoff

The `gravity_scout_miss_count` property on Tension nodes tracks consecutive investigations that found no gatherings:

```cypher
SET t.gravity_scouted_at = datetime(),
    t.gravity_scout_miss_count = CASE
        WHEN $found_gatherings THEN 0
        ELSE coalesce(t.gravity_scout_miss_count, 0) + 1
    END
```

Target selection uses this for backoff:
- 0 misses: re-scout after 7 days
- 1 miss: 14 days
- 2 misses: 21 days
- 3+ misses: 30 days (capped)

**Antifragile property:** Success resets the counter to 0. A tension that suddenly starts creating gravity (crisis escalation, viral moment) gets picked up within 7 days because the miss count resets. The system adapts in both directions — backs off when cold, snaps back when hot.

### Temporal decay and signal freshness

Gatherings are temporal. A vigil series can stop. A weekly singing rebellion might end after a few months. The system handles this through two mechanisms:

1. **Existing reap mechanism** — `reap_expired_signals` removes stale nodes based on `last_confirmed_active`. Gravity-discovered signals get the same timestamp as any other signal.

2. **Touch on dedup** — When the gravity scout re-visits a tension and finds a gathering still active, it hits the dedup path and calls `touch_signal_timestamp` to refresh `last_confirmed_active`. When it doesn't find the gathering again, the node naturally ages out.

### Integration point

Runs in `Scout::run_inner()` after the response scout and before story weaving:

```
Clustering → Response Mapping → Curiosity Loop → Response Scout → Gravity Scout → Story Weaving → Investigation
```

### Budget

Per tension investigated:
- 3 Claude Haiku calls (investigation + structuring, may terminate early)
- 5 Tavily searches (early termination uses ~2-3)
- 3 Chrome page reads

The scout checks budget availability before starting and skips if exhausted.

## Key Design Rationale

### Why separate from response scout?

The prompts are fundamentally different. Response scout asks "what solves this?" Gravity scout asks "where are people showing up?" Combining them in one prompt would dilute both — the LLM would return a mix of instrumental responses and gatherings without clear framing for either.

### Why require `cause_heat >= 0.1`?

Cold tensions don't create gatherings. Nobody holds a vigil for a tension that hasn't manifested. The heat threshold ensures the gravity scout only targets tensions that are active enough to pull people together.

### Why 7-day re-scout (not 14)?

Gatherings are temporal and fast-moving. A new vigil series might start mid-week. The singing rebellion might add new locations. 7 days keeps the system responsive to gathering dynamics. (The response scout uses 14 days because instrumental responses change more slowly.)

### Why fewer targets (3 vs 5)?

Gravity is rarer than instrumental response. Most tensions don't have visible gatherings — only the hottest, most active ones do. Investigating 3 hot tensions deeply is better than spreading thin across 5.

### Why `gathering_type` on the edge (not a new edge type)?

A property on RESPONDS_TO is the simplest way to distinguish while keeping one edge type. Downstream code can filter by `gathering_type IS NOT NULL` to find gravity edges. A new edge type would require changes across the entire codebase — reader, API, UI — for no structural benefit.

### Why no emergent tension discovery?

The gravity scout's job is narrow: find gatherings. Tension discovery is the curiosity loop's job. The response scout discovers emergent tensions because it investigates deeply enough to stumble on new problems. Gravity investigation is more surface-level — "find events" — and adding tension discovery would complicate the prompt without much payoff.

### Why `is_recurring` matters?

A one-time vigil is meaningful. A weekly singing rebellion is a sustained community formation — evidence that the tension's gravity is strong enough to create ongoing structure. Recurring gatherings are the strongest signal of community crystallization.

### Why embedded triage (not a separate call)?

A separate triage call decides based on the tension *description alone* — it would miss unexpected gravity like tenant solidarity potlucks for "housing affordability." Embedded triage lets the LLM look at actual search results before deciding, catching surprises while still terminating early (2-3 Tavily calls) when there truly is nothing.

### Why exponential backoff capped at 30 days?

A fixed 7-day window wastes budget on tensions that never create gatherings. Backoff (7→14→21→30 days) is antifragile: it adapts to reality. Capped at 30 because civic dynamics shift faster than 90 days — a dormant tension can erupt into protests within a month.

### Why venue seeding?

A church that hosts a singing rebellion likely hosts other tension-related events. Creating future query sources for discovered venues means each successful investigation compounds — the system gets better at finding gatherings over time, not just repeating the same searches. "First Baptist Church Minneapolis community events" is specific enough to avoid generic collisions.

### Why `also_addresses` edges use `create_gravity_edge`?

If the gravity scout used `create_response_edge` for cross-tension wiring, those additional edges would lack `gathering_type`. A singing rebellion that addresses both "ICE fear" and "housing instability" should have `gathering_type = "singing"` on both edges, not just the primary one.

## Structured Output Types

```rust
pub struct GravityFinding {
    pub no_gravity: bool,                  // True → early termination
    pub no_gravity_reason: Option<String>, // Why the LLM stopped early
    pub gatherings: Vec<DiscoveredGathering>,
    pub future_queries: Vec<String>,
}

pub struct DiscoveredGathering {
    pub title: String,
    pub summary: String,
    pub signal_type: String,       // "event", "give", "ask"
    pub url: String,
    pub gathering_type: String,    // Freeform: "vigil", "singing", "solidarity meal"
    pub venue: Option<String>,     // Where people gather
    pub is_recurring: bool,        // Recurring = sustained community formation
    pub organizer: Option<String>, // Who creates the gravitational center
    pub explanation: String,       // How tension creates this gathering
    pub match_strength: f64,
    pub also_addresses: Vec<String>,
    pub event_date: Option<String>,
}
```

## Files

| File | Role |
|------|------|
| `modules/rootsignal-scout/src/gravity_scout.rs` | Main module: struct, prompts, investigation, finding processing |
| `modules/rootsignal-scout/src/curiosity.rs` | Tool types shared via `pub(crate)`: `SearcherHandle`, `ScraperHandle`, `WebSearchTool`, `ReadPageTool` |
| `modules/rootsignal-graph/src/writer.rs` | Methods: `find_gravity_scout_targets`, `get_existing_gravity_signals`, `mark_gravity_scouted`, `create_gravity_edge`, `touch_signal_timestamp` |
| `modules/rootsignal-scout/src/budget.rs` | Budget constants: `CLAUDE_HAIKU_GRAVITY_SCOUT`, `TAVILY_GRAVITY_SCOUT`, `CHROME_GRAVITY_SCOUT` |
| `modules/rootsignal-scout/src/scout.rs` | Integration: runs gravity scout after response scout, before story weaving |
| `modules/rootsignal-graph/tests/litmus_test.rs` | Integration tests: target selection, backoff, edge creation, coexistence with response edges |

## Constants

```rust
const MAX_GRAVITY_TARGETS_PER_RUN: usize = 3;
const MAX_TOOL_TURNS: usize = 10;
const MAX_GATHERINGS_PER_TENSION: usize = 8;
const MAX_FUTURE_QUERIES_PER_TENSION: usize = 3;
```

## Edge Cases

### `no_gravity` + non-empty gatherings

If the LLM sets `no_gravity: true` but also returns gatherings, the system treats gatherings as empty. Don't process partial results from an early-terminated investigation. The contradiction is logged as a warning.

### Cross-tension gravity reveals tension relationships

A gathering that addresses multiple tensions is evidence that those tensions are *connected in the community's experience*. "ICE fear" + "housing instability" might seem unrelated in the graph, but if the same solidarity gathering addresses both, they share gravity. This is captured via multi-tension RESPONDS_TO edges with `gathering_type` — no special handling needed now, but a rich signal for story weaving in the future.

## Future Work

**Story weaver integration.** Stories about tensions that have gravity edges should surface the human response alongside the problem. "People are singing at Lake Street Church" is more powerful than "ICE enforcement is happening."

**Gravity heat contribution.** Gatherings could feed back into `cause_heat` — a tension with many active gatherings is clearly generating community energy, which might be a different signal than the tension itself being severe.

**Venue as first-class entity.** A venue that hosts multiple gravity gatherings across tensions is a community anchor. Lake Street Church, Powderhorn Park, etc. Future work could promote recurring venues to Actor nodes or a new Venue node type.
