# StoryWeaver: Emergent, Anti-Fragile Story Generation

## Problem

Stories were only created by the Leiden clustering pipeline, which requires 10+ connected signals at 0.65+ cosine similarity. For new or small cities, this threshold never fires. Meanwhile, the curiosity engine does real journalism — investigating signals, searching the web, discovering underlying tensions — but only creates isolated Tension nodes with RESPONDS_TO edges. The narrative never became a Story.

## Core Design Decision: Stories Are Graph Patterns, Not Created Objects

A story already exists in the graph the moment a tension accumulates 2+ responding signals from distinct sources. The `Story` node is a **materialized view** — a cached rendering of a graph pattern for the API/UI layer. The source of truth is the tension hub itself.

This means:
- **No special "creation" logic** — just materialize what's already there
- **Stories grow automatically** — every new RESPONDS_TO edge potentially extends a story
- **Stories fade naturally** — when signals age out, the materialized view decays
- **The graph is the memory** — even if a Story node is deleted, the tensions + edges remain and can re-materialize

## Architecture

### Three-Phase Pipeline

```
Phase A: Materialize (always runs, no LLM)
  Tension with 2+ RESPONDS_TO edges → Story node
  Headline = tension title, Summary = tension summary

Phase B: Grow (always runs, no LLM)
  Existing story's tension gets new RESPONDS_TO → link signal to story
  Refresh metadata (signal_count, type_diversity, source_domains)

Phase C: Enrich (budget-gated, uses LLM)
  Stories with synthesis_pending=true → LLM synthesis
  Includes contradiction detection, resurgence context
```

### Data Flow

```
Signal → RESPONDS_TO → Tension
                          ↓ (StoryWeaver Phase A)
                        Story ←CONTAINS→ [Tension, Signal1, Signal2, ...]
                          ↓ (StoryWeaver Phase B, on next run)
                        Story ←CONTAINS→ [Tension, Signal1, Signal2, Signal3_new, ...]
                          ↓ (StoryWeaver Phase C)
                        Story.lede, Story.narrative, Story.arc populated
```

### Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `CuriosityOutcome` | `writer.rs` | Done/Skipped/Failed/Abandoned — tracks investigation lifecycle |
| `TensionHub` | `writer.rs` | Tension + 2+ respondent signals, ready to materialize |
| `TensionRespondent` | `writer.rs` | Per-signal struct with edge metadata (prevents parallel array desync) |
| `StoryGrowth` | `writer.rs` | Existing story + new signals not yet linked |
| `StoryWeaver` | `story_weaver.rs` | Orchestrator: three-phase run() |
| `StoryWeaverStats` | `story_weaver.rs` | Run statistics for observability |

### Graph Queries

**Query A — Materialize new stories:**
```cypher
MATCH (t:Tension)<-[r:RESPONDS_TO]-(sig)
WHERE NOT (t)<-[:CONTAINS]-(:Story)
WITH t, collect({sig_id, source_url, strength, explanation}) AS respondents
WHERE size(respondents) >= 2
```

**Query B — Grow existing stories:**
```cypher
MATCH (t:Tension)<-[:CONTAINS]-(story:Story)
MATCH (t)<-[r:RESPONDS_TO]-(sig)
WHERE NOT (story)-[:CONTAINS]->(sig)
```

## Anti-Fragility Properties

### Contradictions Surface, Not Suppress

When a story's respondent signals include mixed types (Tension + Give, or opposing Notices), the synthesis prompt explicitly asks the LLM to surface the disagreement as multiple perspectives. No pairwise embedding comparisons needed — just check signal type diversity in the respondent set. The LLM does the nuance work; we give it the structural signal that divergence exists.

### Investigation Failures Become Coverage Gap Intelligence

Signals retry up to 3 times. After 3 failures, the outcome transitions to `Abandoned`. The StoryWeaver counts abandoned signals per-run and logs a warning when the count is non-zero. This surfaces blind spots for human attention.

```
Failed (retry 1) → Failed (retry 2) → Failed (retry 3) → Abandoned (permanent)
```

### Resurgence Is a Named Arc

When a fading story (arc = "fading") gets new RESPONDS_TO activity, it doesn't just go back to "Emerging" — it gets arc "Resurgent". Resurgent stories are editorially interesting: something that died came back. The synthesis prompt notes: "This tension was quiet for N days before new activity."

### Budget Exhaustion Degrades Gracefully

Phase A (materialize) and Phase B (grow) are cheap graph queries + writes. They always run. Phase C (LLM synthesis) is budget-gated. A story with no synthesis is still a valid story — it has a headline (tension title) and signals. Enrichment runs when budget returns.

## Emergence Properties

| Property | How |
|----------|-----|
| Stories exist without LLM | Phase A/B create stories from graph structure alone |
| Stories grow organically | Phase B adds new respondents each run |
| Minimum viable story = 2 signals + 1 tension | The graph pattern IS the definition |
| Stories merge via signal overlap | Hub signals already in a story → absorbed, not duplicated |
| Arc evolves from observation | Emerging → Growing → Stable → Fading → Resurgent |

## Relationship to Existing Clustering Pipeline

The Leiden clustering pipeline (`cluster.rs`) and StoryWeaver are complementary, not competing:

- **Leiden** creates stories from signal-signal similarity (SIMILAR_TO edges, cosine distance). It requires 10+ connected signals and works best for mature cities with many signals.
- **StoryWeaver** creates stories from tension-signal response patterns (RESPONDS_TO edges, from curiosity investigations). It works with as few as 2 signals + 1 tension.

Both feed into the same `Story` nodes. The containment check in Phase A prevents duplicates — if a tension hub's signals overlap >= 50% with an existing Leiden-created story, the hub is absorbed rather than spawning a duplicate.

## Files Modified

| File | Change |
|------|--------|
| `modules/rootsignal-graph/src/story_weaver.rs` | **New** — StoryWeaver three-phase pipeline |
| `modules/rootsignal-graph/src/cluster.rs` | Made `story_status()` and `story_energy()` `pub` |
| `modules/rootsignal-graph/src/writer.rs` | Added `CuriosityOutcome`, `TensionHub`, `TensionRespondent`, `StoryGrowth` types; `find_tension_hubs()`, `find_story_growth()`, `count_abandoned_signals()` queries; updated `mark_curiosity_investigated()` and `find_curiosity_targets()` |
| `modules/rootsignal-graph/src/synthesizer.rs` | Added `was_fading` param to `compute_arc()`; added `synthesize_with_context()` with extra editorial context |
| `modules/rootsignal-graph/src/lib.rs` | Registered `story_weaver` module, exported new types |
| `modules/rootsignal-common/src/types.rs` | Added `Resurgent` variant to `StoryArc` |
| `modules/rootsignal-scout/src/curiosity.rs` | Tracks per-tension failures, passes `CuriosityOutcome` |
| `modules/rootsignal-scout/src/scout.rs` | Wired StoryWeaver call after curiosity loop |
| `modules/rootsignal-scout/src/budget.rs` | Added `CLAUDE_HAIKU_STORY_WEAVE` cost constant |

## Future Work (Not in This PR)

- **Story splitting:** Mega-tensions (30+ respondents) get `needs_refinement = true` for a future `story_splitter` pass.
- **Echo meta-stories:** When a story is flagged "echo", create a meta-tension about potential manipulation.
- **Coverage gap tensions:** Auto-create tensions from abandoned signal patterns.
