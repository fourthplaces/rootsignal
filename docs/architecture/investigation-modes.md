# Investigation Modes: Three Irreducible Forces

The scout pipeline has three investigation modes. Each asks one irreducible question that can't be further decomposed or combined with another without losing clarity.

| Mode | Question | Discovers |
|------|----------|-----------|
| **Curiosity Loop** | "Why does this signal exist?" | Tensions — systemic forces creating pressure |
| **Response Scout** | "What diffuses this tension?" | Instrumental responses — legal aid, mutual aid, boycotts, programs |
| **Gravity Scout** | "Where are people gathering around this tension?" | Solidarity — community formation, cultural crystallization, people showing up |

## The Atomic Forces Principle

Each mode captures a distinct force that can't be reduced to the others:

**Curiosity** asks about causation. A food bank signal exists because of food insecurity. An ICE raid report exists because of immigration enforcement. The curiosity loop traces signals back to the structural tensions that create them. This is the *diagnostic* force.

**Response** asks about solutions. Given a tension, what solves or diffuses it? Legal clinics, mutual aid networks, advocacy organizations, policy changes. These are instrumental — they exist to address the problem. This is the *instrumental* force.

**Gravity** asks about gathering. Given a tension, where are people coming together? Not to solve the problem, but because the problem pulls them together. Vigils, singing rebellions, solidarity meals, packed town halls. This is the *solidarity* force.

A "Know Your Rights Workshop" solves a problem (response). A "Singing Rebellion at Lake Street Church" doesn't solve ICE enforcement — it transforms fear into solidarity (gravity). Both matter. Both are real. They require different prompts, different search strategies, and different framing to discover well.

## Why No Third Mode

It's tempting to imagine a "community self-organizing" mode that captures things like benefit concerts, solidarity funds, or community-built alternatives. But these aren't a third force — they're the overlap between the existing two.

A benefit concert for immigration legal defense is:
- **Instrumental** (it raises money for legal aid) → response scout finds it
- **Solidarity** (people show up, gather, express shared values) → gravity scout finds it

The overlap isn't a design problem — it's discovered data. When both scouts find the same signal, the graph naturally captures both relationships: a RESPONDS_TO edge (instrumental) and a DRAWN_TO edge with `gathering_type` (solidarity). The signal sits at the intersection, connected to the tension through both forces.

This is why the architecture hones in on each force in its most atomic, irreducible form and lets the overlap emerge naturally from the data. Designing a third mode for the overlap would mean:
- The prompt would be vague ("find things that are kind of both solving and gathering")
- It would duplicate work the other two scouts already do
- The LLM would return a confused mix instead of clean, well-framed results

The right number of investigation modes is the number of irreducible questions. Three questions, three modes. The connections between them are discovered, not designed.

## What Root Signal Investigates

Root Signal is about **open wounds in the collective that haven't healed**. This scoping principle determines what the investigation modes look for:

**Collective, not individual.** Loneliness is real, but it's an individual condition — not a tension that creates community response. Root Signal surfaces what affects groups of people who might come together around it.

**Systemic, not episodic.** A single hospital closing is an event. Healthcare deserts across rural communities is a tension. Root Signal tracks the structural forces, not individual incidents (though incidents may be signals that reveal tensions).

**Unresolved, not solved.** Tensions persist because they haven't been adequately addressed. When a tension is fully resolved, its heat drops, signals age out, and the system naturally stops investigating it.

This scoping means the gravity scout looks for gatherings around collective wounds — not individual support groups (which might be response scout territory as a service), not one-off events unrelated to tension, but sustained community formation around shared pressure.

## How the Forces Interact

The three modes run in sequence during each scout cycle:

```
Curiosity Loop → Response Scout → Gravity Scout
```

Each builds on the previous:
1. **Curiosity** discovers tensions from raw signals
2. **Response** finds what solves those tensions
3. **Gravity** finds where people gather around those tensions

But the interactions go deeper:

**Response reveals gravity targets.** A tension with many instrumental responses is clearly active — people are building solutions. That same tension likely also creates gatherings. The response scout's activity is a leading indicator for the gravity scout.

**Gravity reveals tension connections.** A gathering that addresses multiple tensions (via `also_addresses`) is evidence that those tensions are linked in the community's experience. ICE fear and housing instability might seem unrelated in the abstract, but if the same solidarity gathering addresses both, they share community infrastructure. This is data that neither mode could produce alone.

**Heat drives attention allocation.** `cause_heat` — computed from signal corroboration across sources — determines which tensions get gravity-scouted. The hottest tensions get investigated first. This means the system naturally gravitates toward the most active community pressures without anyone curating the priority list.

## Gathering Type: Freeform by Design

The `gathering_type` property on DRAWN_TO edges is intentionally freeform — "vigil", "singing rebellion", "solidarity meal", "tenant meetup", "community cleanup". It is not normalized to an enum.

This is deliberate:

**Small sample bias.** The system currently covers a few cities with predominantly urban, social-justice-oriented tensions. Any taxonomy derived from this sample would encode that bias — missing gathering types that emerge in rural communities, environmental contexts, economic solidarity, or cultural traditions the current cities haven't surfaced.

**Emergence over prediction.** The gravity scout discovers gathering types the designers didn't anticipate. Tenant solidarity potlucks, read-aloud events at libraries in response to book bans, community compost days after an environmental spill. A fixed enum would force the LLM to shoehorn these into predefined categories, losing the specificity that makes them valuable.

**Future normalization.** Once the system has accumulated data across many cities and tension types, gathering categories can be derived empirically — clustering freeform strings by embedding similarity to find natural groupings. This produces a taxonomy grounded in reality rather than speculation.

The tradeoff is that aggregation queries ("how many vigils across all cities?") require fuzzy matching rather than exact filtering. This is acceptable at the current scale and becomes solvable once the data supports empirical clustering.

## Pipeline Position

```
Bootstrap → Scraping → Discovery → Clustering → Response Mapping
  → Curiosity Loop → Response Scout → Gravity Scout → Story Weaving → Investigation
```

See also:
- [Curiosity Loop](curiosity-loop.md) — how tensions are discovered from signals
- [Response Scout](response-scout.md) — how instrumental responses are found
- [Gravity Scout](gravity-scout.md) — how gatherings are discovered
- [Multi-City Gravity](gravity-multi-city.md) — how gravity works across cities
