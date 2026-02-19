# Signal Expansion: Every Signal Radiates Discovery Queries

## Core Principle

Every signal the system discovers contains more information than what was extracted. A single signal radiates outward in every direction — population, venue, network, cause, gap, geography, urgency. **Signal expansion** is the principle that the system should extract these embedded queries and use them to drive the next cycle's discovery.

An Ask for emergency bail funds doesn't just imply a Give exists. It implies a population (detained immigrants), a cause (enforcement actions), a geography (is this local or national?), a community response (who's organizing the fundraiser?), actors (who's detaining? who's mobilizing?), and a gap (if they're fundraising for bail, what about legal defense? housing? mental health?). One signal, seven directions.

This is how the discovery flywheel tightens. Each scout run doesn't just find signals — it generates the seeds for the next run's discovery. The landscape gets richer with every cycle because the system expands outward from what it finds.

## The Two Audiences

Root Signal's signal types serve two audiences:

- **Give** serves people who need help. "Free legal consultation, walk-ins welcome." "Emergency food shelf, open Tuesdays." The audience is the person experiencing the tension.
- **Ask** serves people who can contribute. "We need food donations." "Volunteer drivers needed." The audience is the person with resources to give.

The flip between these two audiences is one specific expansion direction: an Ask implies a Give should exist, and vice versa. But signal expansion goes far beyond audience-flipping.

## Per-Signal Expansion

Every signal type radiates queries in multiple directions.

### Ask: "Emergency bail fund needed for detained immigrants in Minneapolis"

| Direction | Expanded query |
|-----------|---------------|
| The need | Bail funds, legal defense funds (Give) |
| The population | "detained immigrants" → services for this population |
| The cause | People are being detained → enforcement pattern (Tension) |
| The urgency | "Emergency" → active right now, current events |
| The community | People are fundraising → gatherings, solidarity (Gravity) |
| The actors | Who's detaining? Who's organizing? |
| The geography | Minneapolis → is this regional or national? |

### Give: "Free legal clinic for Somali immigrants, Tuesdays at Brian Coyle Center"

| Direction | Expanded query |
|-----------|---------------|
| The population | Somali immigrants → other services for this community |
| The venue | Brian Coyle Center → other events at this location |
| The gap | Legal is covered → what about housing? health? employment? |
| The tension | Why do Somali immigrants need free legal help? |
| The network | Who funds this clinic? Who else partners with them? |

### Event: "Packed school board meeting on proposed closures in North Minneapolis"

| Direction | Expanded query |
|-----------|---------------|
| The tension | School closures → deeper investigation |
| The gravity | Packed meeting → more gatherings, organizing |
| The population | North Minneapolis families → services, other events |
| The actors | School board members, parent organizations |
| The response gap | What's being done about closures? |

### Notice: "City approves rezoning of industrial site for luxury condos"

| Direction | Expanded query |
|-----------|---------------|
| The tension | Displacement, affordability |
| The actors | Developers, city council, affected residents |
| The response | Tenant advocacy, affordable housing campaigns |
| The gravity | Neighborhood meetings, protests |

## Collection-Level Expansion

Per-signal expansion radiates from individual signals. But collections of signals reveal patterns that no single signal contains.

### Per-Page: The Shape of a Source

A page with three signals:
- Give: "Free legal clinic for immigrants, Tuesdays"
- Ask: "Volunteer interpreters needed, Spanish and Somali"
- Event: "Know Your Rights workshop, Saturday"

The collection tells you: there's an organized immigration response infrastructure here. The expanded queries are different from anything a single signal would generate:

- **Network**: "Who else is part of this coalition?" → look for partner organizations
- **Trigger**: "What's driving this mobilization?" → look for the enforcement action or policy change
- **Spread**: "Where else is this happening?" → look for similar clusters in other neighborhoods
- **Negative space**: Legal + education covered, but where's emergency housing? Mental health? The gap is information.

### Per-Tension: What's Missing from the Response

All signals responding to "Immigration Enforcement Fear":
- 3 legal clinics (Give)
- 2 Know Your Rights workshops (Event)
- 1 bail fund Ask
- 0 emergency housing
- 0 mental health support
- 0 children's services

The shape of the response reveals what's absent. Three categories of help exist. Three don't. The missing categories aren't visible in any individual signal — they emerge from looking at the collection and asking "what would a complete response look like?"

### Per-Run: Emerging Patterns

Everything discovered in one scout cycle across a city:
- 12 new signals in North Minneapolis, 2 in South, 0 in Northeast
- Heavy clustering around immigration and youth violence
- No signals about environmental issues despite known contamination sites

The run-level view reveals geographic and thematic gaps. "Northeast Minneapolis has an active PFAS contamination site and zero community signals" is itself a discovery query: `"PFAS contamination Northeast Minneapolis community response"`.

## The Scales

| Scale | Input | What it reveals | Where it fits |
|-------|-------|----------------|---------------|
| **Per-signal** | Single extracted signal | Population, venue, network, cause, gap, geography | Extractor — additional output field |
| **Per-page** | All signals from one source | Network shape, organizational clusters, trigger events | Extractor or post-extraction pass |
| **Per-tension** | All signals responding to a tension | Response completeness, missing categories, gap analysis | Discovery engine briefing |
| **Per-run** | Everything discovered this cycle | Geographic gaps, thematic imbalances, emerging patterns | Discovery engine briefing |

## Implementation

Signal expansion is implemented across three layers: per-signal query extraction, immediate vs deferred expansion, and collection-level response shape analysis.

### Per-Signal: Extractor Enhancement

The LLM already reads full page content and extracts signals. Each `ExtractedSignal` now includes an `implied_queries` field — up to 3 search queries that would discover related signals by expanding outward from this one:

```rust
pub struct ExtractedSignal {
    // ... existing fields ...
    #[serde(default)]
    pub implied_queries: Vec<String>,
}
```

The extractor returns an `ExtractionResult` that wraps both the extracted nodes and the collected implied queries:

```rust
pub struct ExtractionResult {
    pub nodes: Vec<Node>,
    pub implied_queries: Vec<String>,
}
```

Cost: zero extra LLM calls. The extractor already processes the content — `implied_queries` is an additional output field in the same JSON response. `#[serde(default)]` means missing or malformed queries silently become empty arrays — signal extraction quality is never degraded.

The prompt instructs the LLM to only generate implied queries for signals with a clear tension connection. Routine community events (farmers markets, worship services) should return an empty array. This is the first layer of noise defense.

### Immediate vs Deferred Expansion

Not all implied queries can be used immediately. The timing depends on whether the signal is *inherently* tension-linked:

**Immediate expansion (Tension + Ask signals):** These signal types are inherently about community tensions. Their implied queries are collected during extraction and used in the current run's Phase B discovery.

**Deferred expansion (Give + Event signals):** These need response mapping first — we don't know if a Give or Event is tension-linked until synthesis wires RESPONDS_TO edges. Their implied queries are stored on the Neo4j node as a native `List<String>` property and expanded *after* response mapping creates the connection.

```
Phase A: Extract signals → collect Tension/Ask queries immediately
         Store Give/Event queries on nodes as implied_queries property

Synthesis: Response mapping wires RESPONDS_TO/DRAWN_TO edges

Post-synthesis: Query Give/Event nodes that are now tension-linked
                Collect their implied_queries, clear the property
                Add to expansion pool
```

The deferred pass uses this Cypher pattern:

```cypher
MATCH (s)-[:RESPONDS_TO|DRAWN_TO]->(t:Tension)
WHERE (s:Give OR s:Event)
  AND s.implied_queries IS NOT NULL
  AND size(s.implied_queries) > 0
  AND coalesce(t.cause_heat, 0.0) >= 0.1
RETURN s.implied_queries AS queries, s.id AS id
```

After collection, `implied_queries` is set to `null` on processed nodes to prevent replay on subsequent runs. Queries survive across runs — if a Give gets linked to a tension in a future run, its queries are still available.

### Heat-Gating

Only signals linked to tensions with `cause_heat >= 0.1` drive expansion. A cold tension (zero heat) doesn't need more discovery — it hasn't been confirmed by multiple sources yet. This prevents the system from expanding into dead ends.

### Dedup: Token-Based Jaccard Similarity

Expansion queries are deduplicated against existing active WebQuery sources using token-based Jaccard similarity (threshold: 0.6), not substring matching.

Why Jaccard, not substring: substring matching kills specific long-tail queries. "emergency housing for detained immigrants" would be discarded if "housing" already exists as a query — they share 1/6 tokens (Jaccard = 0.17, well below threshold). But "housing assistance Minneapolis" vs "housing resources Minneapolis" share 2/3 tokens (Jaccard = 0.67, correctly flagged as duplicate).

```rust
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let a_tokens: HashSet<&str> = a.to_lowercase().split_whitespace().collect();
    let b_tokens: HashSet<&str> = b.to_lowercase().split_whitespace().collect();
    intersection / union
}
const DEDUP_JACCARD_THRESHOLD: f64 = 0.6;
```

### Volume Control

The combined pool of immediate + deferred queries is capped at `MAX_EXPANSION_QUERIES_PER_RUN = 10`. Even with 50 signals generating 3 queries each, only Tension/Ask queries expand immediately (much smaller set), Give/Event queries are heat-gated, and the final pool is deduped and capped.

Sources created from expansion queries use `DiscoveryMethod::SignalExpansion` for provenance tracking. Low-yield expansion sources self-deactivate via the existing weight mechanism after 10 consecutive empty runs.

### Collection-Level: Response Shape Analysis

The discovery engine briefing now includes **response shape per tension** — what types of responses exist and what's absent. This feeds the LLM better information about where the gaps actually are.

```rust
pub struct TensionResponseShape {
    pub title: String,
    pub what_would_help: Option<String>,
    pub cause_heat: f64,
    pub give_count: u32,
    pub event_count: u32,
    pub ask_count: u32,
    pub sample_titles: Vec<String>,
}
```

The discovery prompt renders this as actionable gap analysis:

```
## RESPONSE SHAPE (what's missing from each tension's response)

- "Immigration Enforcement Fear" (heat: 0.80)
  What would help: legal defense, emergency housing, mental health support
  Gives: 3, Events: 2, Asks: 1
  Known: "ILCM Legal Clinic", "Know Your Rights Workshop", "ICE Rapid Response Fund"
  → GAP: all Gives are legal — search for housing, mental health, children's services
```

This works in concert with per-signal expansion: individual signals radiate outward in all directions, while the collection-level view tells the discovery LLM where the negative space is densest.

### Observability

Signal expansion adds three stats to `ScoutStats`:

| Stat | What it measures |
|------|-----------------|
| `expansion_queries_collected` | Total implied queries collected from extraction (Tension + Ask immediate + Give/Event deferred) |
| `expansion_sources_created` | Sources actually created after dedup + cap |
| `expansion_deferred_expanded` | Give/Event nodes whose deferred queries were collected this run |

If the LLM stops generating implied_queries, these stats show zeros — silent degradation is visible.

## The Negative Space Principle

The most important discovery queries aren't always about what the system found — sometimes they're about what's missing. A tension with legal clinics but no mental health support. A neighborhood with contamination but no community signals. A population being served by three organizations but with no emergency housing.

Signal expansion makes negative space visible. By systematically asking "what does each signal imply should exist?" and "what does the collection's shape reveal is absent?", the system generates queries that target exactly what's missing.

This is how the discovery flywheel compounds: signals → expanded queries → more signals → more expanded queries. Each cycle reveals more of the landscape, and the negative space shrinks.

## Relationship to Amplification Scout

The [amplification scout plan](../plans/2026-02-19-feat-amplification-scout-plan.md) proposes an agentic investigation mode that takes tensions with known engagement and searches broadly for all other forms of engagement. Signal expansion overlaps with this at the per-tension collection level — both identify gaps in the response shape and seek to fill them.

The difference is mechanism and timing:

| | Signal Expansion | Amplification Scout |
|---|---|---|
| **Mechanism** | Query generation (feeds next cycle) | Agentic investigation (LLM + tools, same run) |
| **Cost** | Zero extra LLM calls | 2 Haiku + 3 web searches + 2 Chrome per tension |
| **Scope** | Every signal, every scale, every cycle | Per-tension only, 5 per run, 30-day cooldown |
| **Depth** | Generates queries — breadth over depth | Follows threads — depth over breadth |

Signal expansion is the always-on, zero-cost foundation. If it generates good enough queries, the normal pipeline closes most gaps within a few cycles. The amplification scout may still be valuable as a targeted deep-dive for high-heat tensions where the expansion queries aren't sufficient — but signal expansion should be built first. It may make amplification unnecessary, or it may reveal the narrow cases where amplification adds value.

## Relationship to Investigation Modes

Signal expansion is orthogonal to the investigation modes (see [investigation-modes.md](investigation-modes.md)). The modes ask irreducible questions about tensions:

- Curiosity: "Why does this exist?"
- Response: "What solves this?"
- Gravity: "Where are people gathering?"

Signal expansion asks: "What does this signal imply should exist?" It doesn't investigate — it generates discovery queries. Those queries feed the normal pipeline (scraping, extraction, dedup), which feeds the investigation modes. It's the discovery engine's job, not a new investigation mode.

## Relationship to Signal-to-Response Chain

The existing [signal-to-response chain](signal-to-response-chain.md) describes how data flows through the pipeline: Signal → Tension → Response. Signal expansion adds a feedback loop: each discovered signal generates queries that feed the *next* run's discovery. The chain becomes a spiral — each cycle goes deeper because it's guided by what the previous cycle found and what it didn't find.
