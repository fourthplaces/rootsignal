---
date: 2026-02-17
topic: anti-fragile-signal
---

# Anti-Fragile Signal: Letting Truth Emerge Under Pressure

## What We're Exploring

How to make Root Signal structurally resistant to astroturfing, propaganda, misinformation, and institutional manipulation — not by deciding what's true, but by letting reality emerge from the shape of the data itself. Specifically addressing the gap where the most critical civic activity goes underground precisely when it matters most.

## Key Insights

### 1. Remove Source Trust Entirely

The current confidence formula weights `source_trust` (0.3) based on domain TLD (.gov=0.9, .org=0.8, social=0.3). This is fundamentally flawed in a stressed system: the highest-trust domains may be the source of the crisis (federal agencies sending troops), while the lowest-trust sources (community social media) may be closest to ground truth.

**Proposal:** No source is trusted. The graph is trusted. Each signal is a snapshot of observable behavior ("Organization X publicly said Y at time T about place P"), not a claim about truth. The Evidence node with content hash proves publication, not veracity.

### 2. Triangulation Over Echo

Corroboration by source count is a popularity contest. News agencies parrot each other — 50 outlets reprinting the same AP wire story is echo, not corroboration.

**Real corroboration = type diversity across signal types pointing at the same underlying reality.**

- 50 Notices saying the same thing = **echo** (weak)
- 1 Notice + 1 Event + 1 Give + 3 Asks = **triangulation** (strong)

You can manufacture claims. You can't easily manufacture a coordinated ecosystem of events, asks, gives, and tensions that all cohere.

**Proposed formula:**

```
signal_strength = type_diversity × org_diversity × specificity
```

### 3. Underground Activity Leaves Visible Traces

Grounded in real interview data (see `docs/interviews/2026-02-17-volunteer-coordinator-interview.md`): during the Twin Cities ICE enforcement crisis, the most critical civic infrastructure (churches sheltering families, food distribution networks, daycare protection) went deliberately invisible. Organizations stopped posting on social media because visibility = danger from ICE.

But the activity isn't absent — it's **displaced**. Observable traces include:

- Individuals posting daily about causes (not orgs)
- GoFundMes in personal names, not organizational
- Asks for volunteers with no specific location
- Events announced <24 hours in advance
- Grocery stores in specific neighborhoods seeing unusual demand

**Pattern:** When organizational signal goes quiet but individual signal spikes, something important is happening underground.

### 4. Proxy Signals: The Missing Building Block

The system needs to surface **proxy individuals** — people who have voluntarily chosen to be public-facing for causes they care about. These are the "Volunteer A"s: people who say "I'll take the heat so the org doesn't have to."

Properties of a proxy signal:
- An individual is publicly broadcasting
- They're clearly connected to a cause (not an org)
- They're offering themselves as a contact point
- The system captures their public signal without EVER mapping the network behind them

**The system NEVER traces back to the organizations.** It protects the underground by design, not by policy.

### 5. The Last Mile is Human

The system cannot and should not fully validate everything algorithmically. Its job is to:

1. **Capture the shape** of what's happening (triangulated signals)
2. **Surface the proxies** (individuals who chose to be visible)
3. **Create the connection point** (help → person → trust network)

The final validation is human-to-human contact. The system gets you to the door. A human opens it. No algorithm replaces "do you know so-and-so? Text only, don't call."

This is what makes it anti-fragile: the more pressure the system is under, the more valuable the proxies become. The system doesn't expose the underground — it amplifies the people who've already chosen to stand in front of it.

## Revised Confidence Model

### What We're Replacing

The current model assigns a static `source_trust` score based on domain TLD (.gov=0.9, .org=0.8, social=0.3). This is wrong for two reasons:

1. **Trust isn't a property of a source.** It's a property of a signal's relationship to the rest of the graph. The same .gov source is trustworthy for "library hours" and untrustworthy for "our enforcement operation was justified." The system shouldn't have an opinion about either.

2. **Trust is fuzzy and dynamic.** One day you might trust the government, the next day you might not. A static score baked into source metadata can't capture this. But graph position — how a signal connects to everything else — is inherently fuzzy, inherently dynamic, and inherently resistant to gaming.

### What Replaces It

**Source trust as a concept is replaced entirely by graph position.** A signal's weight comes from how it connects to everything else, not from who said it.

The dimensions that matter:

- **Triangulation** — how many distinct signal types (Event, Give, Ask, Notice, Tension) independently point at the same underlying reality? This is the strongest indicator. You can manufacture claims. You can't easily manufacture a coordinated ecosystem of events, asks, gives, and tensions that all cohere.

- **Specificity** — does the signal have concrete, falsifiable properties? A time, a place, an action? Vague claims are weak. "I'm delivering groceries to 4 families on Tuesday" is specific. "The community needs support" is not.

- **Connectedness** — does the signal fit into an existing cluster of related signals, or is it isolated? Isolated signals aren't necessarily wrong, but they carry less weight until other signals corroborate them.

- **Freshness** — is the signal recent and actively confirmed?

The exact weighting of these dimensions should stay emergent — discovered through real data, not prescribed upfront. The principle is: **trust is earned by fitting the shape of reality, not by having the right domain name.**

### 6. Entities, Not Orgs

The current system uses `org_mappings` as the atomic unit of civic agency — deduplicating organizations across platforms and using `org_count` to drive velocity and story confirmation. But this is hardcoded to the assumption that only organizations have civic agency.

The interview data proves otherwise. An individual volunteer coordinating deliveries, running a GoFundMe, posting daily on Instagram — she has asks, gives, events, tensions. The full signal vocabulary. She's not an org. She's an entity with civic agency.

**Entity replaces org as the atomic unit of agency.** An entity is anything that publicly acts in civic space:

- A church distributing food = entity
- A volunteer coordinating deliveries = entity
- A government agency issuing a notice = entity
- A grocery store donating surplus = entity
- A neighborhood coordinator dispersing resources = entity

They all produce the same signal types. They all get deduplicated across platforms the same way (an individual's Instagram + GoFundMe + Facebook = one entity). They all contribute to triangulation the same way.

The distinction between org and individual becomes **metadata, not architecture.** The graph doesn't treat them differently. An entity is an entity. Whether it's an org or a person is an attribute — useful for some affordances (e.g., gap view might show "3 orgs and 12 individuals responding in this area") but irrelevant to how signals are weighted or clustered.

This also solves the proxy problem cleanly. You don't need a special "proxy signal" building block. A proxy is just an individual entity whose signals triangulate with a cluster of activity. The graph reveals the relationship structurally — you never have to explicitly map "this person is a proxy for that org." The system never creates that trail.

### 7. Signal Utility, Not Newsroom

The system surfaces patterns and evidence. It does not tell stories, editorialize, or generate narratives with a point of view. The temptation is to frame the output as "the actual news" — but Root Signal is a signal utility (see `docs/vision/problem-space-positioning.md`). It makes the shape of civic reality legible. The human interprets the story.

This distinction matters for anti-fragility: a system that generates narratives can be accused of bias. A system that surfaces triangulated, evidence-backed patterns and lets individuals decide what they mean — that's structurally honest. It aligns with the Alignment Machine principle: "The system doesn't define what alignment is. It surfaces pressure points and lets individuals decide."

Narrative generation (e.g., LLM-produced story summaries) is an **affordance** — one way of presenting signal — not the identity of the system. See `docs/brainstorms/2026-02-17-signal-vs-affordances-brainstorm.md`.

### 7. Don't Over-Engineer Defenses

The system should not pre-optimize for hypothetical threats. Build it, watch what happens, adapt. If a threat emerges (e.g., bad actors using the system to target proxies), evolve the system in response — go invite-only with vetting, restrict access patterns, etc. But don't add friction now that slows down real people who need signal amplification today.

This is the core anti-fragile move: the system gets stronger from stress because it adapts to actual threats, not imagined ones. Premature defense is a form of over-engineering that makes the system brittle, not resilient.

## Anti-Fragility Audit

Anti-fragile doesn't mean "survives stress." It means **gets stronger from stress.** For each stress vector, the system should improve under pressure — not just hold up. Here's what's already anti-fragile, and what's missing.

### Already Anti-Fragile

**Stress produces more triangulation.** A community in crisis generates Asks, Gives, Events, Tensions — all at once, from many entities. The more stressed the system is, the richer the triangulation. The picture becomes MORE reliable under pressure, not less. This is the deepest anti-fragile property and it's built into the signal type model itself.

**Orgs go underground → individual entities emerge.** When organizations go dark, the individuals who step forward produce signal the system didn't have before. The graph gets richer, not poorer. The system captures entity-level signal it would never have seen in normal mode.

**Sources die → new sources are born.** Already designed in the self-evolving system (`docs/vision/self-evolving-system.md`). A missing source creates a coverage gap → gap analysis generates new search queries → new sources are discovered → the system ends up with MORE sources than before.

**Media moves on → the system persists.** Signals don't expire because a news cycle ended. They expire based on freshness of the underlying reality. When media moves on from Minneapolis, the Asks, Gives, and Events are still active in the graph. The system becomes the ONLY record of what's still happening. Stress makes it more valuable.

**Astroturfing reveals itself.** Manufactured signal lacks type diversity. 100 fake Notices from coordinated accounts = echo. Real civic activity produces Events + Gives + Asks + Tensions from independent entities. The structural difference between echo and triangulation makes manipulation visible, not hidden.

### Not Yet Anti-Fragile (Mechanisms Needed)

**The system doesn't learn from manipulation attempts.** Triangulation is stateless — each clustering run starts fresh. An anti-fragile system would build memory: not of "who's bad," but of what echo patterns look like structurally. When the system detects echo (high source volume, low type diversity, low entity diversity), that pattern should inform future detection. Each manipulation attempt should make the next one harder to pull off.

Possible mechanism: echo signature detection — when a cluster has high signal count but low type diversity and low entity diversity, flag it as echo. Over time, the system builds a catalog of echo patterns that makes detection faster and more precise.

**The system doesn't detect proxy gaps.** When an active individual entity goes quiet in a stressed area, that's a signal itself. It could mean they're burned out, targeted, or in danger. An anti-fragile system would notice the gap and actively search for other entities broadcasting about the same cause — the same way it discovers new sources when an org's website goes down.

Possible mechanism: entity activity monitoring within active clusters. When an entity that was producing consistent signal goes quiet, trigger discovery for other entities connected to the same cause or geography. The loss of one proxy should accelerate the discovery of others.

**Entity graph position doesn't build over time.** An entity whose signals consistently triangulate with ground truth — whose Gives correspond to real Events, whose Asks match real Tensions — should naturally gain graph position over time. An entity whose signals consistently fail to triangulate should naturally lose graph position. Not as a trust score (that's what we're removing), but as emergent consequence of track record.

Possible mechanism: signal triangulation rate — over time, what fraction of an entity's signals end up in triangulated clusters vs. remaining isolated? This is not source trust. It's an emergent property of how an entity's signal relates to the rest of the graph. It's dynamic (changes as the graph changes), fuzzy (not binary), and can't be gamed without producing real civic activity.

**Government misinformation doesn't have consequences.** A .gov source that produces Notices which never triangulate with ground-level activity is currently treated the same as one that does. An anti-fragile system would let this mismatch accumulate. Over time, entities whose signals don't connect to observable reality (no corresponding Events, Gives, or Asks from independent entities) naturally occupy weaker graph positions. The system doesn't punish them — it just stops amplifying signals that don't connect to anything real.

### The Core Anti-Fragile Property

All of the above mechanisms share one principle: **stress produces information, and the system captures that information to improve itself.**

- Manipulation attempts → teach the system what echo looks like
- Proxy loss → triggers discovery of new proxies
- Source death → triggers discovery of new sources
- Government misinformation → reveals entities whose signal doesn't connect to reality
- Community crisis → produces the richest, most triangulated signal in the entire graph

A fragile system breaks under these stresses. A resilient system survives them. An anti-fragile system uses each one as fuel to become more accurate, more connected, and more useful.

## Resolved Questions

- **Should proxy signals be a new node type?** No. The entity model solves this. `org_mappings` becomes `entity_mappings`. An entity is anything with civic agency — org or individual. Proxies are just individual entities whose signals triangulate with a cluster. No special building block needed.

- **How do we handle surfacing proxies vs. protecting them?** Respect their agency. They chose to be public. The system amplifies signal that's already public — it doesn't create new exposure. If threats emerge, adapt then (invite-only, vetting) — don't pre-optimize for hypothetical problems.

- **How does the narrative affordance stay honest?** It's a pattern view, not a newsroom. The system surfaces triangulated evidence and lets humans interpret meaning. Root Signal is a signal utility. Narrative is one affordance, not the identity. See `docs/brainstorms/2026-02-17-signal-vs-affordances-brainstorm.md`.

## Open Questions

- How do we detect "displaced signal" patterns (entity silence + individual spike) algorithmically? The anti-fragility audit proposes entity activity monitoring, but the concrete implementation needs design.
- What does echo signature detection look like in practice? How does the system build memory of echo patterns without reintroducing source-level trust?
- How does entity graph position accumulate over time without becoming a static trust score? The signal triangulation rate concept needs stress-testing.
- What's the migration path from `org_mappings` to `entity_mappings`? How do we ingest individual entities from platforms currently scraped only for org accounts?
- What does the LLM extraction prompt look like for individual-voice signal? ("I'm delivering groceries" vs. "we're hosting an event")

## Next Steps

→ `/workflows:plan` to design the entity model, revised confidence model, and individual entity ingestion pipeline
→ See `docs/brainstorms/2026-02-17-signal-vs-affordances-brainstorm.md` for affordance framing
