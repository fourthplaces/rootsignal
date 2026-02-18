---
date: 2026-02-17
topic: triangulation-model
---

# Triangulation Model: From Echo to Structural Truth

## What This Document Is

A pressure-tested analysis of how triangulation should work in Root Signal, derived from close reading of the vision docs, the anti-fragile brainstorm, the volunteer coordinator interview, and the actual codebase. This document captures the reasoning, the scenarios that shaped the design, and the specific problems each piece solves.

## The Core Insight

Echo and triangulation are different *shapes*, not different *magnitudes*.

```
ECHO:                                TRIANGULATION:

  Notice ─┐                           Tension ──┐
  Notice ──┤── same claim repeated     Ask ──────┤
  Notice ──┤   from many sources       Give ─────┤── different types,
  Notice ──┤                           Event ────┤   same underlying
  Notice ─┘                           Notice ───┘   reality

  5 sources, 1 signal type            5 sources, 5 signal types
  = popularity contest                = structural corroboration
```

You can manufacture claims. You can't easily manufacture a coordinated ecosystem of events, asks, gives, and tensions that all cohere. A fake campaign can flood notices. It can't simultaneously produce a real food shelf with hours, a real volunteer shift, a real GoFundMe from a real person, and a real city council hearing.

This is the deepest anti-fragile property of Root Signal's signal type model: **stress produces more triangulation.** A community in crisis naturally generates all five signal types at once from many independent entities. The more pressure the system is under, the richer the triangulation. The picture becomes MORE reliable under pressure, not less.

## What Already Exists in the Code

The building blocks are present but not wired together:

- **`type_diversity`** is computed per story in `cluster.rs:138` and stored on `StoryNode`. It counts unique signal types (Event, Give, Ask, Notice, Tension) within each Leiden-detected community. But it's not used for ranking or status determination.

- **`cause_heat`** in `cause_heat.rs` computes cross-story attention spillover: `Σ(cosine_similarity × neighbor.source_diversity)` for similar signals. This is valuable for a different reason — a food shelf Ask rises when the housing crisis is trending — but it's type-blind. A signal surrounded by 10 identical-type neighbors looks the same as one surrounded by 5 different types.

- **`source_diversity`** counts distinct entity sources per signal, updated during corroboration. Already resists self-promotion (posting 30 times from the same org doesn't inflate it).

- **`entity_count`** on stories counts distinct entities across all signals in the cluster.

- **Story status** is currently `if entity_count >= 2 { "confirmed" } else { "emerging" }` — but a story with 20 entities all posting Notices is echo, not confirmation.

- **Story energy** is `velocity * 0.5 + recency * 0.3 + source_diversity * 0.2` — no triangulation component.

- **Signal ranking** in `reader.rs` sorts by `cause_heat DESC, confidence DESC, last_confirmed_active DESC`. No awareness of whether the signal lives in a well-triangulated cluster.

- **Source trust is gone from the scoring pipeline.** No `.gov = 0.9` in the Rust code. `confidence` is purely extraction quality (completeness, geo, freshness). The template `node_detail.html` still renders `source_trust_label` — a ghost reference that should be cleaned up.

## Scenarios That Shaped the Design

### Scenario 1: The ICE Enforcement Crisis (from volunteer coordinator interview)

**What happens:** Federal enforcement operations begin in the Twin Cities. Churches shelter families. Food distribution networks activate. Daycare centers stop taking children outside. Organizations go deliberately silent on social media because visibility = danger from ICE.

**What the graph looks like:**
- Tension: ICE enforcement operations in South Minneapolis
- Ask: "Volunteers needed for food delivery" (individual voice, GoFundMe in personal name)
- Ask: "Daycare needs support — families can't leave their homes"
- Give: Church A distributing food boxes weekly (public Instagram)
- Give: Grocery store owner donating surplus (mentioned in individual's posts)
- Event: Restaurant solidarity dinner (announced <24 hours in advance)
- Notice: Know-your-rights training (legal aid org website)

**Type diversity = 5/5. Entity diversity = 7+.** This is maximally triangulated. No single source is saying "crisis" — but the shape of the activity screams it. The system doesn't need to detect a crisis. Triangulation surfaces it structurally.

**Why this matters for design:** The triangulation score must live at the cluster level. Individual signals don't know they're part of a triangulated pattern. The cluster does. A lone Ask ("we need grocery volunteers") is just an ask. That same Ask inside a cluster with Tensions, Gives, Events, and Notices is part of a triangulated picture of civic reality.

### Scenario 2: Astroturfing Campaign

**What happens:** A coordinated campaign creates 30 fake community organizations, each with a website, each posting one Notice about the same manufactured issue.

**What the graph looks like:**
- 30 Notice signals from 30 different domains
- All semantically similar (same framing, same claims)
- Zero Asks, zero Gives, zero Events, zero Tensions
- No corresponding real-world activity

**Source diversity = 30. But type diversity = 1/5.**

With the current system (no triangulation scoring), this cluster would look legitimate: high source_diversity, high cause_heat (30 similar signals boosting each other), confirmed status (entity_count >= 2).

With triangulation scoring, this cluster has `type_diversity = 1` — structurally classified as echo. No amount of source manufacturing can fix this because you'd need to simultaneously create real events with real times and places, real asks with real GoFundMes people can donate to, real gives with real operating hours. You can't fake an ecosystem.

**Why this matters for design:** Echo detection is a cluster-level observation, not a signal-level computation. A cluster with `type_diversity == 1` and `signal_count >= 5` is echo. Period. This should be flagged as `status = "echo"` and ranked below emerging stories.

### Scenario 3: The Food Shelf and the Housing Crisis

**What happens:** A small food shelf posts once a week. Separately, a housing crisis is generating dozens of signals — eviction notices, legal aid events, tenant organizing, GoFundMes for displaced families.

**What the graph looks like:**
- Story A (Housing): Tension + Ask + Give + Event + Notice (type_diversity = 5)
- Story B (Food shelf): Give (type_diversity = 1)
- The food shelf's embedding is semantically close to the housing cluster (poverty connects them)

**What should happen:** The food shelf Give should be boosted — not because it's triangulated (it's not; it's a single Give), but because the *cause it serves* has intense community attention.

**This is what cause_heat already does.** `cause_heat(food_shelf) = Σ sim(food_shelf, housing_signal) × housing_signal.source_diversity`. The food shelf rises because its semantic neighborhood is hot.

**Why this matters for design:** Cause heat and triangulation are different things. Cause heat = cross-story attention spillover. Triangulation = within-story type diversity. They must remain separate. An early version of this analysis proposed evolving cause_heat to include a type_bonus multiplier. That was wrong — it would conflate two distinct dimensions and weaken both.

### Scenario 4: Normal-Mode Volunteer Cluster

**What happens:** Several organizations post volunteer opportunities for the same Saturday park cleanup.

**What the graph looks like:**
- 8 Event signals from 5 different orgs
- type_diversity = 1 (all Events)
- entity_count = 5

**Is this echo?** Not maliciously — these are real events from real orgs. But the triangulation is weak: we only know people are *gathering*, not what tension they're responding to, what else is needed, or what resources are available.

**What should happen:** Status = "emerging" (not confirmed). The cluster is real but not triangulated. If an Ask ("we need 20 more volunteers for Saturday") or a Tension ("invasive species threatening native plantings") appears in the same cluster, type_diversity rises and the story strengthens.

**Why this matters for design:** Low type_diversity doesn't mean fake. It means incomplete. The status model should be: echo (low diversity + high volume), emerging (low diversity + normal volume), confirmed (high diversity + multiple entities). Echo is the only suspicious state.

### Scenario 5: Media Echo Chamber

**What happens:** A local news story gets picked up by 15 outlets. Each outlet's coverage gets scraped. All produce Notice signals with similar content.

**What the graph looks like:**
- 15 Notice signals
- type_diversity = 1
- source_diversity = 15 (different media domains)

**This is echo.** Real corroboration isn't 15 outlets reprinting the same AP wire story. Real corroboration is when the news story (Notice) is accompanied by community responses: legal aid clinics opening (Give), community meetings being called (Event), GoFundMes launching (Ask), residents naming the pattern (Tension).

**Why this matters for design:** Source diversity alone cannot distinguish echo from triangulation. 15 sources saying the same thing is weaker than 5 sources each contributing a different signal type. This is why type_diversity must be a first-class ranking dimension.

## Decisions Made

### 1. Triangulation lives at the story level, not the signal level

**Why:** Triangulation is a property of how signals relate to each other within a cluster. Individual signals don't know they're triangulated. The cluster reveals it. Computing triangulation at the signal level (like cause_heat does with pairwise similarity) would require mixing type-awareness into an already complex computation and would conflate two different concepts.

**What this means:** `StoryNode.type_diversity` (already computed, already stored) IS the triangulation score. No new computation needed — just wire it into ranking and status.

### 2. Don't touch cause_heat

**Why:** Cause heat solves a different problem (cross-story attention spillover). It's type-blind by design — a food shelf should rise when housing signals are hot, regardless of signal type. Adding type awareness to cause_heat would break this property and conflate two dimensions.

**What this means:** Cause heat continues to work as-is. Triangulation is a new, separate ranking dimension that flows from stories to signals.

### 3. No formulas — use the raw graph observation

**Why:** Principle 13 says "prefer graph structure over application logic." Type diversity is a graph observation: how many distinct signal types exist in this cluster? It's not a formula. It's a count. Using it directly (`type_diversity = 4` means 4/5 types are present) is more honest and more resistant to gaming than wrapping it in coefficients.

**What this means:** Story energy should include `type_diversity / 5.0` as a component. Signal ranking should incorporate story type_diversity as a sort dimension. No `type_bonus` multipliers.

### 4. Echo is a status, not just low triangulation

**Why:** A cluster with `type_diversity = 1` and `signal_count >= 5` has a specific structural signature: many signals of the same type. This could be legitimate (many orgs posting events for the same cleanup) or suspicious (coordinated notices). Either way, it's not confirmed — it's echo. Making "echo" an explicit status communicates this to downstream consumers and enables separate handling (investigation, damped ranking).

**Threshold:** `type_diversity == 1 AND signal_count >= 5` → `status = "echo"`.

### 5. Story status uses both entity diversity AND type diversity

**Why:** The current status (`entity_count >= 2` = confirmed) doesn't capture triangulation. A story confirmed by multiple entities of the same type is not structurally different from echo with extra sources. Real confirmation requires both: multiple entities AND multiple signal types.

**New status logic:**
- `entity_count >= 2 AND type_diversity >= 2` → "confirmed"
- `type_diversity == 1 AND signal_count >= 5` → "echo"
- Everything else → "emerging"

### 6. Defer the entity model change

**Why:** The anti-fragile brainstorm proposes replacing `org_mappings` with `entity_mappings` where individuals have equal standing. This is real and important — the volunteer coordinator interview proves it. But it's a separate, larger piece of work (cross-platform individual dedup, individual-voice extraction prompts, entity type metadata). Triangulation at the type level works without it and delivers immediate value.

**What this means:** Individual entity tracking is the next step after triangulation scoring is live. When it lands, the entity_count dimension of triangulation becomes more accurate (Volunteer A's Instagram + GoFundMe resolve to one entity instead of two).

## What This Enables

**Anti-astroturfing without blacklists.** Echo detection is structural. No editorial decision about "who's bad." The graph shape reveals it.

**Crisis surfaces naturally.** Real crises produce all 5 signal types from many entities. Triangulation scoring makes these clusters dominate the ranking without any "crisis mode" toggle.

**Media echo is damped.** 15 outlets reprinting the same story produce a type_diversity = 1 cluster. One outlet plus one community response produces type_diversity = 2. The community response matters more than the media volume.

**Normal-mode signal still works.** A single food shelf Give with no surrounding cluster still surfaces via cause_heat (if its cause is hot) and confidence (if it's well-extracted). Triangulation is additive — it boosts well-triangulated clusters, it doesn't penalize individual signals.

**The system gets stronger under stress.** More community pressure → more signal types activated → higher triangulation → more reliable picture. This is the core anti-fragile property.

## Open Questions

- **Should echo status trigger investigation?** If a cluster is flagged as echo, the Investigator could examine the sources for institutional depth. Low diversity + no evidence = likely astroturfing. Low diversity + real orgs = legitimate event cluster.
- **How does triangulation interact with the Gap View affordance?** A cluster with Tensions and Asks but no Gives or Events represents an unmet need. That's a gap in triangulation — and it's itself a valuable signal. The absence of response types reveals where community capacity is lacking.
- **Should type_diversity weight change per type?** Currently all types count equally. But a Tension (names the underlying issue) + Ask (names what's needed) might be structurally more meaningful than two Gives. This could be over-engineering — start with equal weights and let data inform.
- **Migration path for entity model:** When individual entity tracking lands, how does entity_count change? Currently it uses `resolve_entity()` which falls back to domain extraction. Individual Instagram accounts from hashtag discovery resolve to `instagram.com` as the entity. This should eventually resolve to the individual's username as the entity.

## Related Documents

- `docs/brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md` — the foundational anti-fragility analysis
- `docs/brainstorms/2026-02-17-cause-heat-brainstorm.md` — why cause_heat is a separate dimension
- `docs/brainstorms/2026-02-17-signal-vs-affordances-brainstorm.md` — triangulation is the system; presentation is the affordance
- `docs/brainstorms/2026-02-17-community-signal-scoring-brainstorm.md` — source_diversity replacing raw corroboration
- `docs/brainstorms/2026-02-17-individual-signal-discovery-brainstorm.md` — entity model needed for full triangulation
- `docs/interviews/2026-02-17-volunteer-coordinator-interview.md` — the real-world scenario that grounds the design
- `docs/vision/self-evolving-system.md` — trust as evidence, defense by absence
- `docs/vision/principles-and-values.md` — Principle 13: emergent over engineered

## Next Steps

-> Implementation plan for wiring type_diversity into story status, story energy, and signal ranking
-> Clean up ghost references (source_trust_label in template, source_trust in testing docs)
-> After triangulation ships: entity model evolution (individuals as first-class entities)
