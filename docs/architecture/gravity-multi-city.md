# Heat Is Heat: Multi-City Gravity and Emergent Community Maps

## Core Principle

When the gravity scout selects targets, it picks the hottest tensions globally — not per city. A Palestine tension discovered in Minneapolis with high heat will be gravity-scouted in NYC, Portland, and every other city the system runs in.

This is correct. **Heat is heat.** If something is creating community pressure in Minneapolis, the gravity scout should ask whether it's creating gatherings in NYC too. The tension is global. The gatherings are local.

## The Three Investigation Modes Across Cities

| Mode | Target Selection | Investigation | Output |
|------|-----------------|---------------|--------|
| Curiosity Loop | Per-signal (local) | "Why does this signal exist?" | Tensions (global) |
| Response Scout | Per-tension, lowest response count | "What solves this?" in {city} | Instrumental responses (local) |
| Gravity Scout | Per-tension, highest heat | "Where are people gathering?" in {city} | Gatherings (local) |

Tensions are global. Gatherings are local. The gravity scout bridges the gap — it takes a globally-hot tension and asks what the local community is doing about it.

## How It Works in Practice

### Week 1: Minneapolis bootstraps

Minneapolis runs its first scout cycle. The curiosity loop discovers tensions:
- "ICE enforcement targeting students in their homes"
- "Immigration Enforcement Fear and Community Trauma"
- "Youth Violence Spike in North Minneapolis"

cause_heat runs. Youth violence tensions get high heat (many corroborating signals). The gravity scout investigates the top 3 hottest tensions and finds gatherings — Hub for Nonviolence and Safety, Alternatives to Violence Project workshops, community cookouts, healing circles.

### Week 1: NYC bootstraps

NYC runs its first scout cycle on the same graph. Curiosity discovers NYC-specific tensions:
- "Homeless Encampment Sweeps Reinstatement"
- "Queensboro Bridge Incident"

But these are brand new — they have no heat yet. The globally hottest tensions are still Minneapolis ones from the earlier run.

**The gravity scout investigates Minneapolis tensions for NYC.** It gets "Youth Violence Spike in North Minneapolis" as a target, but the investigation prompt says "in New York City." The LLM searches for NYC youth violence gatherings and either finds them (because NYC has its own youth violence community response) or finds nothing (because the tension is too Minneapolis-specific). Either outcome is fine:
- Found NYC gatherings: great, the system discovered that NYC has its own response to a similar tension
- Found nothing: a few wasted web search calls (2-3 with early termination), moves on

### Week 2+: NYC develops its own heat

After a few cycles, NYC tensions accumulate heat from their own signals, evidence, and corroboration. The gravity scout's target selection naturally shifts — some targets are globally-hot tensions (shared across cities), some are NYC-hot tensions (local). The system self-corrects without any city-specific tuning.

### The Palestine example

A Palestine tension gets discovered in Minneapolis through the curiosity loop — a protest march triggers investigation, the loop finds "Palestinian civilian deaths in Gaza," creates the tension node, heat accumulates from multiple corroborating signals.

When NYC's gravity scout runs:
1. Palestine tension has high heat (globally) — it gets selected as a target
2. Investigation prompt: "Where are people gathering around this tension in New York City?"
3. LLM searches and finds massive NYC gatherings — Columbia encampments, Union Square vigils, Brooklyn marches, mosque gatherings in Jackson Heights, benefit concerts in the Village
4. Those get created as Event/Give/Ask nodes with NYC coordinates
5. DRAWN_TO edges wire them to the Palestine tension with gathering_type ("vigil", "encampment", "march")
6. Venue seeding creates future sources: "Columbia University NYC community events", "Masjid Al-Farooq Brooklyn community events"
7. Next cycle: those venue seeds discover *more* gatherings at those locations
8. Some of those gatherings `also_address` other tensions (Islamophobia, immigration fear) — cross-tension edges form automatically

Nobody programmed "Palestine is relevant to NYC." The heat told the system to look, and the gatherings told the system it was right.

## The Context Anchoring Bug and Fix

### The problem

When the gravity scout investigates a tension for NYC, it passes "existing gravity signals" as context to the LLM — so it knows what's already been found and doesn't waste turns re-discovering known gatherings. But the original implementation returned *all* gatherings for that tension regardless of city. So the LLM's context included "Singing Rebellion at Lake Street Church, Minneapolis" when scouting for NYC, which anchored it toward Minneapolis results.

### The fix

`get_existing_gravity_signals` now filters by geographic bounding box — it only returns gatherings within `radius_km` of the target city's center. When scouting NYC, the LLM sees only NYC gatherings in its context. When scouting Minneapolis, only Minneapolis gatherings.

The target selection remains global (heat is heat). Only the *context* is city-scoped.

### Why not filter targets by city?

Because you'd miss Palestine. A tension discovered in Minneapolis is relevant everywhere. City-filtering the target selection would mean NYC never gravity-scouts Minneapolis-origin tensions, even when those tensions are universal. The cost of investigating a non-transferable tension (a few wasted web search calls with early termination) is far lower than the cost of missing a universal tension with massive local gravity.

## Temporal Dynamics

### Heat sustains attention

As long as a tension has heat (cause_heat >= 0.1) and the gravity scout keeps finding gatherings, the miss_count stays at 0 and re-scouting happens every 7 days. New vigils, new marches, new fundraisers get discovered each cycle.

### Backoff releases attention

When a tension cools — gatherings stop, signals age out, heat drops — the miss_count climbs and backoff kicks in (7 -> 14 -> 21 -> 30 day intervals). Budget shifts to hotter tensions. The system doesn't waste cycles on tensions that aren't generating community response anymore.

### Snap-back recaptures attention

If a cooled tension erupts again (new crisis escalation, viral moment), fresh gatherings reset miss_count to 0 and the system snaps back to 7-day cycles. The antifragile property: the system adapts in both directions. It doesn't need to be told "Palestine is important again" — the heat tells it.

### Reaping handles decay

Gatherings that aren't re-confirmed age out via `reap_expired_signals` based on `last_confirmed_active`. A vigil series that ended in March won't still be in the graph in June. But a recurring gathering that's re-discovered on each gravity scout cycle gets its timestamp refreshed via `touch_signal_timestamp` — it stays alive as long as it's real.

## Emergence

### Venue seeding compounds

Nobody told the system "Lake Street Church matters." The gravity scout found one gathering there, created a venue seed ("Lake Street Church Minneapolis community events"), and now the system re-queries that venue on future discovery cycles. If the church hosts three more events next month, the system finds them — not because anyone curated a venue list, but because one successful investigation created a seed that compounds.

Over enough cycles, the system builds its own map of community anchors. Churches, community centers, parks, schools that keep appearing are the gravitational centers of community life. That's a dataset that doesn't exist anywhere else — it emerges from the investigation loop.

### Cross-tension edges form without design

A gathering that addresses multiple tensions creates `also_addresses` DRAWN_TO edges to each tension. A singing rebellion that addresses both "ICE fear" and "housing instability" creates edges to both. Nobody designed the connection between ICE fear and housing instability — the data revealed it through shared community infrastructure. The same mosque, the same park, the same organizers appear across multiple tensions.

These cross-tension connections are evidence that tensions are *linked in the community's experience*, even if they seem unrelated in the abstract. Story weaving can surface these connections: "People gathering at Lake Street Church aren't just responding to immigration fear — the same community is also processing housing instability, educational inequity, and food insecurity. The church is the gravitational center."

### The system learns community shape

After several cycles across multiple cities, the gravity scout has built:
- A map of which tensions create gatherings (and which don't)
- A map of where people physically gather in each city
- A map of which tensions share community infrastructure
- A temporal record of which gatherings persist (sustained community formation) vs. which are one-time events

This is a bottom-up portrait of community life. Not what organizations exist (the response scout finds that). Not what problems exist (the curiosity loop finds that). But where people *show up* — physically, emotionally, creatively — when pressure hits. The gravity scout surfaces the human response.

## Cold Start Behavior

A new city's first gravity scout run will investigate globally-hot tensions that may be other cities' local issues. This is expected and self-correcting:

1. **First run:** targets are dominated by existing cities' hot tensions. Some transfer (Palestine, immigration fear), some don't (city-specific incidents). The ones that don't transfer terminate early (2-3 web search calls), wasting minimal budget.

2. **Second run:** the new city's own tensions have gained heat from the first cycle's curiosity loop and cause_heat computation. Target selection shifts toward a mix of global and local tensions.

3. **Third run and beyond:** steady state. The city has its own hot tensions, its own venue seeds compounding, its own gathering map forming. Global tensions still get investigated — that's the feature, not the bug.

No tuning required. The system converges to the right behavior through the natural dynamics of heat accumulation and backoff.

## Known Limitations and Future Work

### Target limit per run (currently 5)

The gravity scout investigates 5 tensions per cycle, same as the response scout. Embedded triage makes misses cheap (2-3 web search calls with early termination), so the budget impact of investigating tensions without gatherings is minimal.

### Freeform gathering_type

The `gathering_type` property on DRAWN_TO edges is freeform — "vigil" vs "candlelight vigil" vs "memorial vigil" are different strings for the same type. This is good for emergence (the LLM isn't constrained to a taxonomy) but noisy for aggregation. If downstream code needs to count "how many vigils across cities," it would need normalization. A future enhancement could cluster gathering_types by embedding similarity.

### Venue as first-class entity

Currently, venues are just future query seeds — strings that create WebQuery sources. A venue that hosts multiple gatherings across tensions is a community anchor and arguably deserves its own node type (Place or Venue) in the graph. This would enable queries like "what are the top 10 community anchors in Minneapolis?" and "which venues serve multiple tensions?" The data is already there in the venue seeds; the graph model just doesn't reify it yet.
