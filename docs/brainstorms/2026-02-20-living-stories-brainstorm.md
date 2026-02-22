---
date: 2026-02-20
topic: living-situations
---

# Living Situations with LLM-Driven Weaving

## What We're Building

Replace the mechanical StoryWeaver pipeline with an LLM-driven approach where situations are living megathreads. Each situation has a **living banner** (headline, lede, current state) and a **chronological thread** of dispatches tied to signals. The LLM handles clustering, synthesis, and boundary judgment in one continuous pass.

The name "Situation" is deliberate. It tells the LLM exactly what to do: describe a situation, track its evolution, surface what's needed. No narrative instinct, no dramatic impulse. A situation is a thing that is happening — not a story being told.

## What Is a Situation?

A situation is **root cause + affected population + place**. Not topic + place.

Two eviction waves in Austin are only the same situation if they share a root cause. Riverside evictions driven by short-term rental flipping and East Austin evictions driven by rezoning-induced tax hikes are different situations — different mechanisms, different actors, different responses needed — even though they look like "housing displacement in Austin" on the surface.

The situation boundary is **causal**, not geographic or topical. The theme emerges from the cause, not the other way around.

### Situations as Rally Points

A situation isn't just journalism — it's a **rally point**. One root cause, one situation, one place to organize around. When a chemical plant explodes, the evacuation, the contamination, the hospital overflow — those aren't separate situations. They're cascading effects of one cause, documented as thread entries under one situation.

This means thread entries do more than narrate. They surface **what's needed**: Aid signals, Need signals, Gathering signals — contextualized within the situation. The situation is the umbrella. The thread entries are on-ramps to action.

### The What vs. The Why — Separating Situation from Cause

A situation is anchored to **what's happening** (to whom, where). The "why" is tracked as evidence within the situation, not as its identity — because causes get politicized, misconstrued, and weaponized.

The thread documents competing causal claims and what the evidence actually supports:

> *"Housing Displacement in East Austin"*
> *Primary evidence points to: rezoning-driven tax increases*
> *Also attributed to: immigration pressure (3 sources), market correction (2 sources)*

The system isn't editorializing. It's showing what different sources claim about the cause, and what the evidence supports — letting people see the gap.

## Situation Lifecycle

Situations are thermodynamic — they have temperature, not mortality. A situation never dies, it just cools.

- **Emerging** — first signals detected, root cause may be unclear
- **Developing** — active signals arriving, causes sharpening
- **Active** — sustained attention, multiple actors, clear root cause
- **Cooling** — signal flow slowing, stabilizing
- **Cold** — no recent signals, but the situation persists and can rewarm

Temperature is **primarily computable, with LLM nuance layered on top**. The computable components are transparent and inspectable:

- **Signal velocity** — new signals per time period
- **Source diversity** — unique independent sources producing signals
- **Amplification** — external geographic references
- **Response coverage** — ratio of unmet tensions to active responses within the situation

The LLM adds qualitative judgment — did the arc shift? is this a new phase? — but cannot override the math. If signal velocity drops to zero, the situation cools regardless of how narratively interesting the LLM finds it. Every ranking factor is visible to the user.

A cold situation can reactivate naturally. If housing signals reappear in the same neighborhood 3 months later and trace to the same root cause, the LLM assigns them to the existing situation and writes a thread entry that documents the return:

> *"After three quiet months, new eviction filings have appeared in the 78702 zip code..."*

No special reactivation logic. The thread itself captures the dormancy and return.

## Situations Start Fuzzy and Sharpen

Early in a situation's life, the root cause may be unclear. The LLM makes a provisional judgment — "these seem related" — and refines as evidence accumulates. This is natural: a journalist starts covering "evictions in Austin" as one beat, then realizes there are two different forces at work.

**Splits** happen when signals that were grouped together turn out to have different root causes. They were never actually the same situation — there just wasn't enough information yet to know that.

**Merges** happen when separate-looking situations turn out to share a root cause. Two neighborhoods, one private equity firm.

The LLM handles splits and merges inline as part of every weaving pass — not as a separate periodic review. It's already reading signals and situation summaries. "Does this still belong here?" is implicit in "where does this belong?" The thread entries document the change in understanding:

> *"What appeared to be separate displacement pressures in Riverside and East Austin now appear connected. Both trace to acquisitions by [firm], which has purchased 340 units across both neighborhoods since October..."*

## Signals Can Associate with Multiple Situations

A single signal can be evidence in more than one situation. The rule is explicit reference — the LLM only associates a signal with a situation if the source explicitly talks about that place/issue.

- Someone in Portland says "what's happening in Austin is terrifying" → **Austin situation only**. It's amplification — external attention on a local situation.
- Someone says "this is happening here too, we're seeing the same thing in Portland" → **both Austin and Portland**. Two explicit references, two situation associations.
- Someone in Portland talks only about Portland → **Portland situation only**, even if the root cause looks similar to Austin's.

The system follows what people are actually saying, not what it thinks they should be connecting. The LLM doesn't infer geographic parallels on its own.

## Amplification

When a local situation gets referenced from outside its geography, that's **amplification**. It's a signal about salience — this issue has broken out of its local context and become a reference point for people elsewhere.

Amplification raises situation temperature. A situation being discussed from 4 other cities is hotter than one with only local attention, even if the local signal count is the same.

The banner can surface this:

> *"Housing Displacement in East Austin"*
> *Signals from 4 cities referencing this situation*

This tells local users their issue has broader attention — meaningful context for organizing and response.

Temperature thus has two components: **local heat** (signals originating within the geography) and **amplification** (signals referencing this situation from elsewhere).

## Causal Chaining — Situations on Situations

Situations don't exist in isolation. They form a **causal graph**.

"Evictions in 78702" traces to "rezoning decision by council" which traces to "developer lobbying campaign" which traces to "state preemption of local zoning." Each hop is its own situation — its own affected population, its own thread, its own temperature, its own rally point. But they're linked by causal edges.

The graph has two layers:

- **Signal → Situation**: signals are evidence within situations
- **Situation → Situation**: situations are caused by / contribute to other situations

A user lands on "Evictions in 78702" and sees the thread. They ask "why is this happening?" and follow the causal chain one hop to the rezoning situation. That situation has its own thread, its own signals, its own actors. They can keep pulling.

### Specificity Through Zoom, Not Limits

The specificity floor isn't a hard cutoff — it's a **zoom level**. Each situation stays specific and actionable at its level. The chain provides broader context without collapsing everything into one mega-situation. "Capitalism" isn't a situation. But you might reach it after 6 hops, and the path there *is* the insight.

This is the curiosity loop operating at the situation level. "Why does this signal exist?" becomes "why does this situation exist?" — and the answer is another situation.

### Cross-Situation Patterns as Coalition Infrastructure

Situation-to-situation links surface connections people can act on:

> *"3 active situations in Austin trace back to the same 2024 council rezoning vote"*

That's not a mega-situation. It's a **pattern made visible through linked situations**. Each keeps its own thread, its own rally point. But people fighting evictions in 78702 can see they share a root cause with the small business closures on East 7th. That's coalition-building infrastructure.

### Same Place, Different Cause = Different Situation

If displacement signals reappear in 78702 a year later but trace to a different mechanism (Airbnb conversions instead of tax hikes), that's a **new situation**. Same neighborhood, same surface effect, different cause — different responses needed, different actors, different rally point.

The two situations can link as related ("previous displacement in this area") without merging. The dual embedding architecture makes this mechanically possible: narrative embeddings match (same place, same effect), causal embeddings diverge (different mechanism), LLM creates a new situation.

## Dual Embedding Architecture

Track two separate embedding spaces per situation:

- **Narrative embedding** — what's happening (the situation). Clusters signals by event and impact.
- **Causal embedding** — why it's happening (the mechanism). Clusters by root cause, actors, forces.

This separation enables:

- **Signal-to-situation matching**: embed incoming signal, vector search against situation narrative embeddings, pull top-K candidates. LLM then verifies causal fit. Embeddings provide cross-run memory — solves the slow-drip problem where signals arrive months apart.
- **Situation embeddings sharpen over time**: as signals arrive, the situation's vector representation tightens. Early on it's a vague cluster. By month 6, it's a tight semantic region.
- **Pattern detection across situations**: same causal embedding appearing in multiple cities → systemic issue. Causal claims diverging from evidence → potential propaganda. Same actor in causal embeddings across situations → power mapping.
- **Astroturf resistance**: source diversity tracked alongside embeddings. A situation hot from 50 signals across 50 sources is more credible than 50 signals from 3 sources.

## How It Works

1. **New signals arrive** from a scout run
2. **Embed each signal**, vector search against situation index → top-K candidate situations
3. **LLM weaving pass**: verify causal fit, assign to existing situation or create new
4. **LLM writes thread entries**: dispatches that surface what's needed (Aid, Need, Gathering signals contextualized within the situation)
5. **LLM reassesses**: temperature, banner, causal claims, and situation boundaries (splits/merges) — all in the same pass
6. **Update situation embeddings** (both narrative and causal) with new signal incorporated
7. **Situations sharpen over time** as the curiosity loop reveals root causes and embeddings tighten

## Situation Structure

- **Banner**: headline, lede, current arc, temperature — rewritten by LLM when the arc shifts
- **Thread**: chronological dispatches, variable length (one sentence for a single signal, a paragraph for a batch), each linking to the signals that informed it
- **Structured State**: root cause thesis, key actors, arc state, timeline of major shifts, causal claims inventory — the LLM's working memory (not user-facing)
- **Signals**: atomic evidence with sources — serve as both raw intelligence and the "footnotes" for thread entries

## Stress-Tested Design Principles

These emerged from pressure-testing the model against edge cases:

- **Contradictions documented, not resolved**: when signals conflict, the thread documents both and flags the discrepancy. The situation tracks reality, including when reality is messy.
- **Event date vs. scrape date**: the weaving pass receives both so the LLM can distinguish retrospective coverage from new developments.
- **Emerging situations from single signals**: create them, mark Emerging, let them prove themselves. No minimum threshold — don't miss early warnings.
- **Structured state over narrative summarization**: for long-running situations (50+ thread entries), the LLM works from a structured state object + recent entries, not a narrative summary. Prevents drift.
- **Cross-scope situations**: local situations can link to national/global situations through actor and causal edges. A national coalition forming in response to local housing fights becomes its own situation when it generates enough signals.

## Vision Alignment — Pressure Tests Against Core Principles

Tested the situation model against every vision doc. Here's what held, what needed hardening, and how the model was reinforced.

### The LLM Is a Lens, Not a Source (Emergent Over Engineered)

The signals are the emergent layer. The LLM doesn't invent situations — it recognizes patterns that already exist in the signal graph and makes them legible. Like a journalist naming a trend that already exists in the data. The emergence is in the signal. The authorship is in the narration.

**Hard rule**: the LLM never speculates beyond the evidence. If it creates a situation, that situation must be grounded in actual signals. If it writes a dispatch, every claim must link to a specific signal. The system breaks its epistemology if the LLM ever becomes a source rather than a lens.

### Temperature Is Transparent (Serve the Signal, Not the Algorithm)

Temperature cannot be a black box LLM judgment. Per vision principle: "every ranking factor is visible to the user." Temperature is decomposable into computable components (signal velocity, source diversity, amplification, response coverage) with LLM qualitative nuance layered on top. The computable components handle ranking. The LLM adds context. Users can see why a situation is hot.

### Resolution Is Honest (The Alignment Machine)

The alignment machine's power comes from the system getting quiet when reality gets quiet. If signal velocity drops to zero, the situation cools — the LLM cannot keep a situation warm because it's narratively interesting. The computable temperature components enforce thermodynamic honesty. The LLM narrates the cooling, but doesn't fight it.

### Temperature and Clarity Are Different Axes (Heat = Understanding)

A situation has both **temperature** (activity level) and **clarity** (how well the root cause is understood). These are orthogonal:

- **Hot + fuzzy**: crisis with lots of signals but unclear cause (emerging)
- **Hot + sharp**: well-understood active situation with clear root cause
- **Cold + sharp**: well-understood historical pattern, gone quiet
- **Cold + fuzzy**: thin signals, unclear cause, hasn't developed

The banner should convey both. "Hot but unclear" tells users: something is happening, but we don't yet know why. This preserves the tension gravity principle that heat = understanding — low clarity means incomplete understanding, regardless of signal volume.

### Dispatches Are Grounded, Not Editorial (No Political Positions)

This is the deepest tension in the model. The LLM writes dispatches. Dispatches imply framing. Framing is editorial. The system was designed to be a mirror.

**Hard rules for dispatches:**
1. Every claim cites a specific linked signal. Users can verify every sentence.
2. Competing causal claims are presented side by side, never resolved by the LLM.
3. The LLM is explicitly instructed to describe, not interpret. "3 new eviction filings this week. 1 mutual aid response launched. City council vote scheduled Feb 28." Not "the housing crisis deepens."
4. Tone is invitational and factual, per editorial principles. Urgency is about opportunity windows, not threats.

The dispatch can be readable and coherent without being editorial — as long as every claim is grounded and competing interpretations are visible.

### Causal Chaining Maps to Power Scout (Power Analysis)

The situation-to-situation causal graph is exactly what the Power Scout vision describes. A local displacement situation links to a rezoning situation which links to a lobbying situation. Power relationships become navigable through causal edges. And the "just raising relationships" principle maps to "what, not why" — the situation tracks what's happening, power relationships are evidence about why.

### Situations Feed Back Into Discovery (Self-Evolving System)

**Gap identified**: the brainstorm didn't originally specify how situations feed back into the discovery layer. Now explicit:

- An **emerging situation with thin coverage** triggers the curiosity engine to investigate deeper (same as signals do today, but at the situation level)
- A **hot situation** accelerates scraping cadence for related sources
- A situation's **causal thesis** generates investigation targets for the investigator
- Situations with **unmet responses** (tensions without matching Aid/Gathering signals) become high-priority in the discovery briefing

This preserves loops 5 (curiosity engine), 6 (unmet tensions → discovery), and 8 (type imbalance) — now operating at both signal and situation level.

### Tension-Response Mechanics Preserved Inside Situations (Feedback Loops)

The current 21 feedback loops are the system's nervous system. The situation model must not flatten them.

**Key preservation**: situations contain tensions and responses as internal structure. A situation's temperature still derives from the tension-response dynamic within it — tensions that have responses cool the situation, unmet tensions heat it up. The RESPONDS_TO edge mechanics are preserved inside the situation's structured state. Situations are a narrative and organizational layer on top of the existing graph — not a replacement for it.

### Anti-Astroturf Stance Is Structural (Defense by Absence)

Situations add a new attack surface: **narrative injection** through coordinated causal framing. The defense is structural:

1. The situation is anchored to **what's happening** (the situation), not why (causal claims)
2. Competing causal claims are documented, never adopted as identity
3. Source diversity is a computable temperature component — single-source flooding doesn't move the needle
4. The dual embedding architecture enables future detection of unnaturally tight causal claim clusters (coordinated framing detection — punted but architecturally supported)

## Key Decisions

- **LLM is a lens, not a source**: recognizes patterns in signals, never speculates beyond evidence
- **Situation = root cause + affected population + place**, not topic + place
- **What, not why**: situation anchored to what's happening; competing causal claims are evidence within, not identity
- **Temperature is decomposable**: computable components (velocity, diversity, amplification, response coverage) primary; LLM qualitative judgment secondary. Every ranking factor visible to the user.
- **Temperature and clarity are different axes**: hot+fuzzy, hot+sharp, cold+sharp, cold+fuzzy are all valid states
- **Temperature, not mortality**: situations cool but never die. Computable components enforce thermodynamic honesty — the LLM cannot keep a situation warm against the math.
- **Dispatches are grounded, not editorial**: every claim cites a signal, competing claims presented side by side, tone is invitational and factual
- **Dual embeddings**: narrative (what) and causal (why) tracked separately per situation
- **Embedding-first matching**: signals find candidate situations via vector search, LLM verifies causal fit
- **Causal chaining**: situations link to other situations, forming a navigable causal graph. Maps to Power Scout vision.
- **Situations feed back into discovery**: emerging situations trigger curiosity engine, hot situations accelerate scraping, unmet responses become discovery priorities
- **Tension-response mechanics preserved**: RESPONDS_TO edges and heat flow exist inside situations, not flattened by them
- **Splits/merges happen inline** as part of every weaving pass
- **Situations start fuzzy and sharpen** as root causes become clear
- **Source diversity** as a temperature and credibility input
- **Subscriptions punted** for now
- **Coordinated framing detection** punted — dual embeddings enable it later

## Open Questions for Planning

- Token budget per weaving pass — how many candidate situations can the LLM reason about?
- Graph model for ThreadEntry nodes, multi-situation signal associations, causal edges between situations, internal tension-response structure, and dual embeddings
- Embedding model choice — same Voyage model for both narrative and causal, or different?
- Structured state schema — what fields does the LLM need to maintain continuity? Must include: root cause thesis, key actors, arc state, clarity level, timeline of major shifts, causal claims inventory, unmet response gaps
- Temperature computation — exact formula for computable components, and how LLM qualitative judgment layers on top
- Dispatch prompt design — how to enforce grounded, non-editorial tone while keeping dispatches readable
- Situation → discovery feedback — how situations integrate into the existing curiosity engine briefing
- Specificity floor in prompts — guidance to keep situations at the most proximate actionable cause
- Migration — lean toward reprocessing existing signals through new pipeline
- Banner rewrite heuristic — every pass or only on arc shifts?

## Analogy

Think situation room meets investigative journalism. One canonical place per root cause per place. When you fly into a city, you see the living situations — each one connects dots that no single source is connecting, tracks the evolution from beginning to now, and shows you the actual evidence. Situations are things people can rally around and eventually subscribe to. Follow the causal chain and you see the bigger picture. Find the linked situations and you find your coalition.

## Next Steps

→ `/workflows:plan` for implementation details
