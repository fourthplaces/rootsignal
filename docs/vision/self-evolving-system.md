# Root Signal — Self-Evolving System

## The Core Idea

Root Signal is not a scraper with a source list. It is an organism that grows its own senses.

The system discovers its own sources by analyzing what it already knows. Signals in the graph reference organizations, programs, and communities that aren't yet tracked. Story clusters reveal gaps — thin corroboration, missing audience roles, geographic blind spots. The system identifies these gaps, generates search queries to fill them, and evaluates what it finds through the same anti-fragility mechanisms it applies to everything else.

Sources earn trust through evidence — when the system investigates and finds institutional depth (registrations, media coverage, grant records, verifiable presence), trust accumulates. Sources with no evidence trail naturally rank low. No blacklists. No editorial gatekeeping. Just evidence.

## How It Works

### The Feedback Loop

```
Seed sources (curated by humans — the bootstrap)
    → Scrape, extract, embed, deduplicate
        → Signal graph (asks, gives, events, notices)
            → Clustering (Leiden community detection → stories)
                → Gap analysis (LLM reads the landscape)
                    → "We have food security signals from 3 orgs but none from the
                        Somali mutual aid networks mentioned in 4 signals"
                    → Generates search queries + candidate URLs
                        → New Source nodes in the graph
                → Investigation (system asks WHY)
                    → New tension about contamination → checks EPA records,
                        city council minutes, news coverage
                    → New discovered source → checks 501(c)(3), media mentions,
                        grant records, physical presence
                    → Evidence nodes accumulate in the graph
                    → Evidence IS trust
                        → Next run scrapes discovered sources alongside the seeds
                            → Loop continues, system expands and deepens
```

Each cycle, the system sees more and understands more deeply. What it sees tells it where to look next. What it investigates tells it what to trust.

### Why This Is Emergent

Nobody tells the system what the themes of a city are. Nobody curates the full source list. Nobody decides which audience roles matter. The system discovers all of this from the signal itself.

- **Stories emerge** from semantic clustering of individual signals
- **Source gaps emerge** from analyzing what stories exist and what they're missing
- **Trust emerges** from evidence accumulation — the system investigates, and evidence IS trust
- **Coverage emerges** from filling gaps that the system itself identifies

The human's role shifts from "decide what to scrape" to "set the geographic scope and let the system fill it in."

### Why This Is Anti-Fragile

Traditional systems get weaker when attacked. This system gets stronger.

**Astroturfing resistance:** A coordinated campaign creates fake sources to flood a story. But:
- New sources start with low trust (no evidence trail)
- Story velocity is driven by *org diversity*, not raw signal count — flooding from one org doesn't move the needle
- The Investigator examines new sources and finds *nothing* — no 501(c)(3), no media mentions, no institutional history
- The *absence* of evidence is the detection signal — you can't fake institutional depth

**Source failure resilience:** A source goes offline, changes its URL structure, or stops producing content. The system doesn't break — it just stops seeing signals from that source. After 10 empty runs, the source is deactivated. Meanwhile, the gap analysis notices the coverage hole and discovers replacement sources.

**Bias correction:** The initial seed sources embed the curator's worldview. But the system immediately starts correcting for this. Signals from seed sources mention orgs that aren't in the seed list. The gap analyzer notices. New sources are discovered. Over time, the source list reflects the actual community landscape, not one person's mental model of it.

## The Newspaper Metaphor

Think of Root Signal like a newspaper that writes itself.

- **Signals** are the raw facts — individual events, resources, needs, notices. Like wire service dispatches.
- **Stories** are the emergent narratives — clusters of related signals that together tell a larger story. Like front-page articles that synthesize multiple reports.
- **Sources** are the newspaper's network of correspondents and stringers. Some are established (the AP, Reuters — analogous to .gov sites, established nonprofits). Some are new (a local stringer who just started filing — analogous to a discovered Instagram account). The good ones earn trust by consistently filing accurate, corroborated reports. The unreliable ones fade.

The key difference: a traditional newspaper's editorial board decides which stories matter and which sources to trust. Root Signal lets those decisions emerge from the evidence. A story matters because multiple independent sources are producing signals about it. A source is trusted because investigation reveals institutional depth — real registration, real history, real presence. Trust is literally the evidence in the graph.

## What "Good Signal" Means

A source produces good signal when its output passes three tests:

1. **Is it community signal?** Relates to community life, ecological stewardship, community engagement, ethical consumption, or the tensions that animate them.
2. **Is it grounded?** Traceable to an identifiable organization, government entity, public record, or established community group. Has an evidence trail.
3. **Does it connect to action or context?** Enables someone to act (volunteer, donate, attend, advocate) or helps them understand what's happening in their community.

Signal that passes all three tests and gets corroborated by independent sources is the gold standard. The system doesn't need to understand *why* something is good signal — it just needs to observe that corroborated, actionable, grounded signal consistently comes from certain sources and not others.

## The Investigation Loop

The system doesn't just sense — it investigates. When the scout pipeline detects something interesting — a new tension, a newly discovered source, a high-urgency signal with thin evidence — the Investigator asks WHY.

This is the critical second loop. The first loop (sense) discovers what's happening. The second loop (investigate) asks why it's happening and whether it's real.

The Investigator follows evidence chains:
- A newly discovered community organization → Is it a registered 501(c)(3)? Does it appear in media coverage? Are there government grant records? Does it have a verifiable physical presence?
- A tension signal about environmental contamination → Are there EPA records? City council minutes? News coverage? Other organizations responding?
- A high-urgency ask for disaster relief → Is there a FEMA declaration? Are mutual aid networks mobilizing? Are other sources reporting the same disaster?

Each verified fact becomes an **Evidence node** in the graph — a first-class entity with a source URL, content hash, retrieval timestamp, and snippet. Evidence nodes link to the signals they support via `SOURCED_FROM` edges.

The investigation doesn't judge. It just follows the trail and records what it finds. The evidence accumulates. And that evidence IS the trust.

## Trust as Evidence

Trust is not a formula. Trust is evidence.

Every source starts with a baseline trust based on its domain type (.gov = 0.9, .org = 0.8, social media = 0.3). This is the system's prior — its initial guess before any investigation has occurred.

Then the Investigator goes to work. It examines signals from the source, follows evidence chains, and produces Evidence nodes. Over time, a source's trust converges toward its evidence density — the ratio of Evidence nodes supporting its signals to the total signals it has produced.

This dissolves the hardest problem in trust modeling: **the corroboration paradox.** A niche nonprofit serving the Hmong community might produce perfectly valid signals that no other source ever mentions. Under a corroboration-rate model, this source would be penalized for being the only voice covering an underserved community. But under evidence-based trust, the Investigator checks: Is this a real 501(c)(3)? Does it have a history of community programs? Are there media mentions, grant records, a physical address? The evidence exists or it doesn't. No other source needs to corroborate — the institutional depth speaks for itself.

Trust never reaches zero. Per the principle: "Root Signal will not gatekeep what enters the graph." A source with no evidence has low trust, but its signals still exist in the graph — they just rank low in confidence-weighted queries. If evidence later accumulates (the Investigator eventually examines that source's signals), trust rises. The system adapts.

## Defense by Absence

The most powerful anti-manipulation mechanism is structural: the system detects bad actors not by what they produce, but by what's *missing*.

Real community activity has a signature:
- Multiple independent organizations talk about it
- It shows up across platforms (website, social media, news)
- It has a history (connected to prior signals in the graph)
- It connects to other signals (part of a story cluster)
- **When investigated, institutional depth is found** — 501(c)(3) registrations, media mentions, government grants, verifiable physical presence

Astroturfing has the opposite signature:
- High signal volume from few sources
- Signals that don't get corroborated by independent orgs
- No history in the graph
- Isolated — not connected to existing story clusters
- **When investigated, there's nothing there** — no registration, no media trail, no institutional history, no verifiable presence

The investigation loop makes this defense active rather than passive. The system doesn't just wait for corroboration to happen — it goes looking for evidence. And for real organizations, evidence is everywhere. For fake ones, it isn't.

The system doesn't need a spam filter. The graph structure *is* the filter, and the Investigator makes it sharper. You can't fake institutional depth — you'd need to plant matching records across 501(c)(3) databases, media archives, government grant listings, and physical directories simultaneously.

## What This Enables

Once the system discovers its own sources, several things become possible that weren't before:

**City bootstrapping:** Point the system at a new city with a minimal seed (5-10 URLs, a few Tavily queries). The system discovers the rest. What took days of manual research becomes a few scout runs.

**Temporal adaptation:** When a crisis hits, the gap analyzer notices the surge in related signals and discovers new sources covering the crisis (mutual aid networks, legal aid hotlines, community response organizations). The system's attention naturally shifts to where community energy is concentrating.

**Bias visibility:** By tracking which sources are discovered vs. curated, the system can report on its own blind spots: "These 12 sources were discovered by the system, not in the original seed list. They produced 23% of all immigrant-audience signals." This makes source selection bias visible rather than hidden.

**Community as sensor:** In future iterations, community members can suggest sources (via web form, email intake). These enter the same trust pipeline — human-suggested sources earn trust through corroboration, same as LLM-discovered ones. The system's sensory apparatus becomes a collaboration between AI and community.

## Principles

1. **Sources are outputs, not inputs.** The curated list is a seed, not a boundary.
2. **Trust is evidence.** Not a formula, not a score — the literal Evidence nodes in the graph. Investigation produces evidence; evidence IS trust.
3. **Nothing is gatekept.** Low trust affects surfacing priority, never admission to the graph.
4. **Absence is signal.** Investigation reveals what's missing — no registration, no media trail, no institutional depth.
5. **The system corrects its own biases.** Gap analysis looks for what's missing. Investigation verifies what's found.
6. **Emergence over engineering.** Don't hard-code what the system can discover.
7. **Humans set scope, not content.** Geographic boundaries and signal domain — the system fills in the details.
8. **The system asks WHY.** Sensing is not enough. Investigation follows evidence chains and records what it finds.
