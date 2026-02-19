---
date: 2026-02-19
topic: resource-capability-matching
---

# Resource Capability Matching

## What We're Building

A new **Resource** node type in the graph that enables matching between what people can offer and what organizations need. Someone with a car finds every org that needs drivers — across all tensions, all signal types. Someone looking for food finds every org giving food.

Resources are a third clustering axis, orthogonal to tensions (the systemic *why*) and stories (emergent narratives). Resources answer: **"what kind of help?"**

## Why This Approach

The current graph clusters around tensions and stories — great for understanding *what's wrong* and *what's happening*. But a person showing up with a car doesn't care which tension they're helping with. They care that someone needs a driver. The graph has no concept of this today.

We considered:
- **Topic-based grouping** (food, transportation, legal) — too coarse. A food bank and a delivery driver are both "food" but the capability is completely different.
- **Need-based taxonomy** — closer, but framed only from the org side. Resources work from both sides: orgs need them, people offer them.
- **Resource nodes** (chosen) — a capability/resource that both Asks and Gives connect to. Creates a natural matching surface via graph traversal.

## Key Decisions

- **Resource nodes are emergent, with seed grounding**: The LLM extractor proposes resource types (canonical label + slug). A seed vocabulary of ~20 common resources grounds the LLM ("use these labels if they fit; otherwise, propose a new concise noun-phrase slug"). This reduces synonym drift without constraining emergence of niche community resources.

- **Three edge types with attributes**:
  - `Requires` — Ask/Event → Resource (must have this to help). Edge properties: `confidence: f32`, `quantity: Option<String>`, `notes: Option<String>`
  - `Prefers` — Ask/Event → Resource (better if you have it, not required). Edge properties: `confidence: f32`
  - `Offers` — Give → Resource (this is what we provide). Edge properties: `confidence: f32`, `capacity: Option<String>`

  The Resource node is a *type* (e.g. "vehicle"). Context like "10 people, Saturday mornings" or "500 lbs shelf-stable protein" lives on the edge, not the node. Clean separation.

- **Fuzzy-AND matching**: Compound needs use multiple Requires edges. Discovery returns anything matching *any* required resource. Ranking uses match completeness:
  - Full match on all `Requires` = 1.0 base score
  - Partial match (1 of 2 Requires) = 0.5
  - Each matched `Prefers` = +0.2 bonus
  - Sorted by score. No separate "also helpful" section — mixed and ranked.

  This avoids "zero-result" traps when resource nodes are granular. A Spanish speaker is still 50% of the solution for "Spanish-speaking driver."

- **Fully orthogonal to tensions**: Resources don't compete with the tension→story axis. They enrich it. Cross-tension queries like "what resources does the housing crisis need most?" fall out naturally: `Tension <--RespondsTo-- Ask --Requires--> Resource`, then aggregate.

- **People are not graph nodes**: A person with a car queries for `Resource(vehicle)` at the API layer. The matching is a graph traversal with location filtering (signals already carry `GeoPoint` lat/lng), not a new node type for users.

- **Daily batch consolidation**: Resource node dedup and merge runs as a daily batch job — not during extraction. Keeps the real-time extraction path fast. Identifies clusters of high-similarity nodes, picks the canonical label (highest `signal_count`), and re-points edges. Fits the existing pattern (cause_heat computed in batch, story weaving runs post-pipeline).

## Pressure Test: Real Scenarios

**"I have a car"** — `Resource(vehicle) <--Requires-- Ask`:
- Second Harvest Heartland: "volunteers to deliver food" → Requires(vehicle)
- ISAIAH: "drivers for court date transport" → Requires(vehicle), Prefers(bilingual-spanish)
- Abuelo's Kitchen: "hot meal delivery drivers" → Requires(vehicle)

One query, three orgs, three different tensions. Clean.

**"I need food"** — `Resource(food) <--Offers-- Give`:
- Community Emergency Service: Give("emergency food assistance") → Offers(food)
- Second Harvest: Give("free groceries") → Offers(food)

**"I speak Spanish"** — `Resource(bilingual-spanish) <--Requires-- Ask`:
- NAVIGATE MN: "interpreters at ICE check-ins" → Requires(bilingual-spanish)
- Centro de Trabajadores: "bilingual workshop volunteers" → Requires(bilingual-spanish)
- ISAIAH: "court date drivers" → Prefers(bilingual-spanish) — surfaces as partial match, score 0.2

**Cross-tension aggregation** — "what resources does the housing crisis need most?":
- `Tension("housing crisis") <--RespondsTo-- Ask --Requires--> Resource`
- Aggregate: physical-labor: 5, vehicle: 3, legal-expertise: 2

**Partial match** — "I have a car but don't speak Spanish":
- ISAIAH Ask needs Requires(vehicle) + Prefers(bilingual-spanish)
- Match score: 1.0 (vehicle) + 0.0 (no Spanish) = 1.0. Still a full match — Spanish is Prefers, not Requires.
- Compare: Ask needing Requires(vehicle) + Requires(bilingual-spanish) → score 0.5 for car-only person. Surfaces but ranked lower.

## Architecture Fit

**Fully additive** — no existing edges or semantics change. `Requires`, `Prefers`, `Offers` are new edge types to a new node type. Existing `RespondsTo`, `DrawnTo`, `GathersAt`, `ActedIn`, `SimilarTo` are untouched.

**Three extraction points**:
1. **Main Extractor** (`extractor.rs`) — add `resources_required` and `resources_offered` to `ExtractedSignal`. Each entry: `{ slug, label, edge_type, confidence, context }`. The LLM already extracts `what_needed` for Asks; resources formalize this.
2. **ResponseScout** (`response_scout.rs`) — add resource tags to `DiscoveredResponse`. Responses that address tensions naturally have resource semantics.
3. **GravityScout** — skip. Gatherings are about solidarity and community formation, not resource matching.

**GiveNode needs no structural change**: Resource tags come from extraction and connect via `Offers` edges. The existing `what_needed` freeform field on AskNode and Resources are complementary — `what_needed` is human context ("10 volunteers with trucks Saturday mornings"), Resources are machine-matchable graph nodes.

**ResourceNode properties** (minimal):
- `id: Uuid`
- `name: String` (canonical label, e.g. "vehicle", "bilingual-spanish")
- `slug: String`
- `description: String` (optional, LLM-generated)
- `created_at: DateTime<Utc>`
- `signal_count: u32` (how many signals connect to this resource)
- `embedding: Vec<f32>` (for dedup)

**Seed vocabulary** (~20, in extractor prompt):
`vehicle`, `bilingual-spanish`, `bilingual-somali`, `bilingual-hmong`, `legal-expertise`, `food`, `shelter-space`, `clothing`, `childcare`, `medical-professional`, `mental-health`, `physical-labor`, `kitchen-space`, `event-space`, `storage-space`, `technology`, `reliable-internet`, `financial-donation`, `skilled-trade`, `administrative`

**Future potential** (not v1):
- **Resource heat**: count of Requires edges × urgency of connected Asks. Surfaces "most needed resources right now."
- **Resource gap analysis**: tensions with many Asks but few Gives for a resource type → unmet need.
- **Cross-tension resource prism**: shared bottlenecks across siloed tensions (housing + education + food access all need `reliable-internet`).

## Next Steps

→ `/workflows:plan` for implementation details (types, extractor prompt changes, graph writer, API queries)
