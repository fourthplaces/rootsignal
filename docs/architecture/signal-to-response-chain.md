# The Signal → Tension → Response Chain

This document explains how the scout's subsystems compose into a single discovery chain. Each subsystem is documented independently — this doc explains *why they're ordered the way they are* and how data flows between them.

## The Chain

```
Signal ──(curiosity)──▶ Tension ──(response scout)──▶ Response
                                                          │
                                                          ├── Give (resource, service, program)
                                                          ├── Event (gathering, action, rally)
                                                          └── Ask (need, request for help)
```

In a single scout run (`Scout::run_inner()`):

```
Scraping → Extraction → Dedup → Clustering → Response Mapping
    → Curiosity Loop → Response Scout → Story Weaver → Investigation
```

The curiosity loop and response scout run back-to-back. This isn't accidental — it's designed so that a tension born from curiosity can be immediately investigated for responses in the same run.

## Concrete Example

One scout run processes "ICE protest this weekend" from a scraped community calendar:

**Phase: Extraction**
- Chrome scrapes community calendar page
- Haiku extracts: `Event { title: "ICE protest this weekend", summary: "Community rally against ICE enforcement" }`
- Signal stored in Neo4j with confidence 0.7

**Phase: Curiosity Loop**
- Signal selected as curiosity target (Event, no existing `RESPONDS_TO` edge, confidence >= 0.5)
- LLM asks: *"Why does this protest exist?"*
- `web_search("ICE enforcement Minneapolis 2026")` → news articles
- `read_page(article_url)` → details about workplace raids, school attendance drops
- LLM extracts tension: `"ICE enforcement fear causing community withdrawal"` (confidence 0.7)
- Tension written to Neo4j; `RESPONDS_TO` edge wired from protest event to tension

**Phase: Response Scout** (same run, minutes later)
- Tension selected as response scout target (confidence 0.7 >= 0.5, `response_scouted_at` is null)
- LLM asks: *"What diffuses ICE enforcement fear?"*
- `web_search("sanctuary city Minneapolis programs")` → legal clinics, know-your-rights workshops
- `web_search("ICE boycott campaigns Minneapolis")` → economic pressure campaigns
- `read_page(clinic_url)` → free immigration legal services
- Extracts responses: Know Your Rights Workshop (Give), Sanctuary City Legal Fund (Give), Community Vigil (Event)
- Each response written to Neo4j with `RESPONDS_TO` edge to the tension

**Phase: Story Weaver** (same run, minutes later)
- Tension "ICE enforcement fear" now has 4 respondents (1 event + 3 responses) from 3+ sources
- StoryWeaver Phase A materializes this into a Story node
- Phase C (budget permitting) synthesizes a narrative lede

**Result:** One scraped event → one tension → three responses → one story. In a single scout cycle.

## Why This Ordering Works

### Curiosity writes to Neo4j before Response Scout queries it

Both systems use the same `GraphStore` connected to the same Neo4j instance. Curiosity creates tensions via `create_node()` (committed writes), and the Response Scout queries `find_response_scout_targets()` against live Neo4j state. No caching layer or eventual consistency delay — the tension is immediately visible.

### Confidence levels enable the chain

| Source | Confidence | Response Scout Threshold |
|--------|-----------|------------------------|
| Curiosity-discovered tension | 0.7 | >= 0.5 (eligible) |
| Emergent tension (from Response Scout) | 0.4 | >= 0.5 (NOT eligible) |
| Extraction-discovered tension | varies | >= 0.5 (if high enough) |

Curiosity creates tensions at 0.7 — well above the 0.5 threshold. This is intentional: a tension backed by multi-hop web investigation with evidence URLs *should* immediately qualify for response scouting.

Emergent tensions (side-effects discovered during response scouting) get 0.4 — below threshold. This prevents an infinite loop: Response Scout discovers tensions → those tensions trigger more Response Scout runs → those runs discover more tensions → etc. At 0.4, emergent tensions need external corroboration (from a future curiosity run or extraction) before they become response scout targets.

### The `response_scouted_at` null case

Newly created tensions have `response_scouted_at = NULL`. The target selection query coalesces NULL to year 2000:

```cypher
coalesce(datetime(t.response_scouted_at), datetime('2000-01-01'))
    < datetime() - duration('P14D')
```

Year 2000 is always > 14 days ago, so new tensions pass the filter immediately.

## The Virtuous Cycle Across Runs

The chain doesn't just work within a single run — it creates a virtuous cycle across runs:

```
Run 1: Scrape "food shelf expansion" → Curiosity discovers "Northside food desert" tension
Run 1: Response Scout finds 2 food programs → Story materializes

Run 2: Scrape "community garden event" → Curiosity links to existing "food desert" tension
Run 2: Response Scout re-checks tension (14 days later) → finds 3 new programs
Run 2: Story grows with new signals, arc shifts from "Emerging" to "Growing"

Run N: Response Scout discovers emergent tension "volunteer burnout at food shelves"
Run N+1: Curiosity corroborates "volunteer burnout" from another signal → confidence rises to 0.7
Run N+2: Response Scout investigates "volunteer burnout" → finds mutual aid networks
```

Each subsystem's outputs feed the next subsystem's inputs. The graph accumulates causal structure over time.

## Subsystem Boundaries

Each subsystem is deliberately independent:

| Subsystem | Input | Output | Can run alone? |
|-----------|-------|--------|----------------|
| Curiosity Loop | Signals without tension context | Tensions + RESPONDS_TO edges | Yes — tensions are valuable even without responses |
| Response Scout | Tensions above confidence threshold | Gives/Events/Asks + RESPONDS_TO edges | Yes — responses are valuable even without curiosity |
| Story Weaver | Tensions with 2+ respondent signals | Story nodes | Yes — materializes whatever graph structure exists |

This independence means:
- If curiosity is budget-exhausted, response scout still processes existing tensions
- If response scout is skipped, curiosity-discovered tensions still form stories via clustering
- If a tension was created by extraction (not curiosity), response scout still finds its responses

The chain is the *optimal* path, not the *only* path.

## Anti-Patterns the Chain Prevents

### Infinite investigation loops

Emergent tensions at confidence 0.4 < response scout threshold 0.5. The chain has a natural damper.

### Redundant discovery

Curiosity provides the tension landscape as context, so the LLM doesn't rediscover known tensions. Response Scout checks existing responses and tells the LLM what categories are already covered.

### Budget spiraling

Each subsystem checks budget independently. If curiosity exhausts the budget, response scout is skipped gracefully. The pipeline degrades from the expensive end (investigation) toward the cheap end (graph queries).

## Architecture Docs

- [Curiosity Loop](curiosity-loop.md) — signal → tension investigation
- [Response Scout](response-scout.md) — tension → response investigation
- [Story Weaver](story-weaver.md) — tension hubs → materialized stories
- [Scout Pipeline](scout-pipeline.md) — full pipeline mechanics
- [Feedback Loops](feedback-loops.md) — all 21 feedback loops mapped
