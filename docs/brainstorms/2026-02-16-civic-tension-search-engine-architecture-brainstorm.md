---
date: 2026-02-16
topic: civic-tension-search-engine-architecture
---

# Civic Tension Search Engine — Architecture from Scratch

## What This Is

A self-directed knowledge system that continuously explores civic reality across the web, builds a living graph of tensions and responses, and lets humans navigate it. A search engine for civic engagement.

## Core Problem

The web is full of civic signal buried in noise. Protests, food drives, GoFundMes, solidarity events, boycotts, mutual aid, environmental disasters, policy changes — all scattered across 50 platforms and buried in algorithmic feeds that don't care about civic engagement. No single place raises this signal, organizes it around tension, and helps people plug in.

## Fundamental Insight

This is a **knowledge graph that builds itself.** The AI isn't extracting data into tables — it's constructing a living model of civic reality. Every tension is connected to causes, actors, responses, history, geography. The system's job is to grow that graph continuously and autonomously.

## Three Loops

### Loop 1: Sense

The system is always listening. Not scraping a fixed list of sources — *searching*. Given a geographic area, the AI asks: "what tension exists here right now?" It searches the open web, social platforms, government data, fundraising sites — everything. It doesn't wait for a human to add sources. It hunts.

This is closer to a search engine crawler than a feed reader. Breadth-first exploration of civic reality, guided by geography and recency.

### Loop 2: Understand

When the system detects tension, it investigates. Not just "what's happening" but the full causal graph: why, who's involved, what's the history, what policy or event triggered this, who's affected, who's responding. The AI as journalist — following threads, connecting dots, building context.

The output isn't a flat record. It's a **graph node with edges** — this tension is caused by X, connected to Y, being responded to by Z.

**"Follow the money" is a core investigation pattern.** The investigator traces funding and enablement chains — who has the federal contracts? Who provides the technology? Who operates the facilities? Who profits? These become Actor nodes with `funds`, `contracts_with`, `enables` edges. Crucially, these actors are shared across geographies — Enterprise's contract with ICE connects to tensions in every city, not just one.

### Loop 3: Surface

A human points at a place on the map. The system shows them the tension graph for that area. Not a list of articles. A living, structured picture: here are the tensions, here's why they exist, here's everyone responding, here's how you can plug in.

Three views — feed, map, summary — all projections of the same underlying graph.

## Data Model: A Graph, Not Tables

The core data structure is a knowledge graph:

```
Node: Tension
Node: Cause (policy, event, actor, system)
Node: Response (org, individual, action, event)
Node: Actor (org, person, government body, company)
Node: Place (neighborhood, city, region)
Node: Evidence (article, post, filing, dataset)

Edge: caused_by, responds_to, located_in, connected_to,
      sourced_from, affects, organized_by, funds,
      contracts_with, enables
```

Everything is a node. Everything is connected. The AI continuously adds nodes and edges. A user query is a graph traversal: "show me all tensions within 20 miles of this point, and for each one, show me all responses."

**Tensions share actors across geographies.** The corporate funding graph (Enterprise, Palantir, CoreCivic) connects to tensions in every city where those actors operate. A boycott in Minneapolis and a boycott in Austin are responding to the same Actor node. The graph knows that.

## Signal Sources

Three classes of signal, all first-class:

1. **Structured sources** — government databases (USAspending, FPDS, 311), city council agendas, permit filings, court records
2. **Unstructured/web sources** — news, social media, GoFundMe, event platforms, org websites, community forums
3. **Human-reported** — people submit signal directly to Taproot ("my sister is feeding 7 families, here's her Venmo")

Human-reported signal enters the graph with the same standing as AI-discovered signal. An individual with a GoFundMe and the ACLU are both Response nodes — **no hierarchy of legitimacy.**

## AI Architecture: A Swarm of Specialized Agents

Not one big pipeline. Autonomous agents with distinct roles:

- **Scout agents** — continuously searching the web for tension signals in target geographies. Cheap, fast, broad. Running constantly.
- **Investigator agents** — when a scout finds something, an investigator goes deep. Follows the causal chain, the money, the policy trail. Builds the graph edges. Slower, more expensive, thorough.
- **Response discovery agents** — given a tension, find everyone responding to it. Scour GoFundMe, event platforms, org websites, social media. Individuals, grassroots, orgs — all of it. Distinct skill from investigation.
- **Synthesis agents** — generate human-readable summaries, briefings, map annotations from the raw graph. Run on-demand when a user looks at something.
- **Freshness agents** — revisit existing nodes. Is this tension still active? Did this event happen? Is this GoFundMe still open? The graph decays without maintenance. **Freshness matters most for grassroots responses** — they change fast and go stale fast.

These agents are autonomous. They don't wait for a human to trigger them. The system has an **attention budget** — compute allocated across geographies and tensions based on activity level. Hot areas get more scouts. Active tensions get more investigators.

## Interface

Dead simple. A map. A search bar.

- Zoom into anywhere. See tension clusters on the map.
- Tap a cluster. See the tension, the context, the responses.
- Search "food insecurity Minneapolis" or "environmental disaster Ohio" and get a structured result — not 10 blue links, but the *graph* around that tension.
- Every response has an action: a link, a signup, a donation page, a phone number. The system doesn't host these — it links out. It's a search engine, not a platform.

## Pressure Testing: Resolved Questions

### Safety: Protecting vulnerable people

The system maps tension geographically — but that geographic data can be weaponized. A real-time map of where people are seeking help from ICE raids is a surveillance tool that cuts both ways.

**Resolution: Geographic fuzziness as a core feature.** The public-facing graph never surfaces exact locations for sensitive tensions. "South Minneapolis" yes. Specific intersection, no. The system knows precise data internally but the public view is deliberately blurred. The level of blur scales with the sensitivity of the tension — ICE-related tensions get more fuzz than a food drive for a school.

Same tradeoff journalists make when reporting on vulnerable communities.

### Speed vs. Truth: Two-tier surfacing

Civic emergencies happen NOW. Verification takes time. These are in tension.

**Resolution: Two tiers.**

- **Tier 1: Unverified but sourced** — surfaces fast (minutes). Every node shows its source. "This was posted on X 20 minutes ago by @account. Not yet verified." The user sees the raw signal and judges for themselves.
- **Tier 2: Investigated** — surfaces slower (hours). The investigator agent has followed the thread, cross-referenced, built context. The node gets an "investigated" edge with evidence.

The system doesn't hide unverified signal — it labels it. Speed and truth coexist through transparency about confidence.

### Trust Without Gatekeeping: Graph density as trust signal

A GoFundMe linked to a known org, corroborated by news articles, and cross-referenced by multiple community reports has a dense graph around it. A standalone GoFundMe with no connections has a thin graph. The user can see that difference.

**Resolution: Trust emerges from graph density, not from the system making a judgment call.** "This effort is connected to [org], [news article], [3 other community reports]" vs. "This effort was found on GoFundMe, no other connections yet." No ranking, no gatekeeping — just transparency about what the graph knows.

### Editorial Stance: Emergent topics, not predefined categories

Defining what counts as a "tension topic" is itself an editorial choice. Predefining categories encodes bias.

**Resolution: The system doesn't predefine topics.** It detects tension agnostically — any spike in civic activity, any cluster of signals around a theme, any area where the graph is growing fast. Topics emerge descriptively from the graph, generated by the AI as labels, not as a taxonomy. The tension around ICE raids and the tension around a pro-ICE rally both get detected — because they're both civic energy. The user sees both and decides for themselves.

The system's editorial stance: **"there is tension here, here's everything connected to it."** Not what's right or wrong.

### Adversarial Inputs: The graph is its own defense

Human-reported signal is a first-class input, which means it can be abused.

**Resolution: Three layers.**

1. **Signal corroboration.** A single report is one node with thin edges. If 5 people report the same thing, or a news source confirms it, the graph gets dense. Flooding the system requires creating fake corroborating evidence across multiple platforms — much harder than spamming one input.
2. **Source reputation over time.** The system learns which sources produce signal that gets corroborated vs. signal that stays isolated. Sources with repeated unverifiable reports naturally fade in influence. Not a ban — their inputs still enter the graph. They just never grow edges.
3. **Rate limiting and anomaly detection.** 200 reports about the same location in 5 minutes with no corroboration from any other source? Suspicious. Flag it, don't surface until scouts can check.

**The graph itself is the defense.** Fake signal doesn't grow edges. Real signal does.

### Cost: Realistic estimates

**Per metro area (~3.5M people):**

| Component | Daily Cost | Notes |
|-----------|-----------|-------|
| Scout agents (web search) | $10-250 | 1,000-5,000 queries/day |
| Social media monitoring | $50-200 | API costs across platforms |
| Platform scraping (GoFundMe, events) | $5-20 | Mostly compute |
| Signal extraction (AI) | $5-100 | 500-2,000 pages/day |
| Investigation agents (deep) | $5-100 | 10-50 investigations/day |
| Response discovery agents | $5-100 | Similar to investigation |
| Embeddings + graph ops | $1-5 | Pennies per embedding |
| Graph DB hosting | $3-17 | $100-500/month |
| **Total per metro** | **$84-792/day** | **$2,500-24,000/month** |

Starting conservative: **$2,000-5,000/month** for one city.

**National coverage at reasonable depth: $20,000-50,000/month** — attention budget drives costs. Quiet areas get occasional checks. Active tensions get heavy coverage.

**Cost management:**
- Scouts use cheap, fast models (Haiku-class) — just detecting "is something happening here?"
- Investigators use expensive models (Opus/GPT-4o) only when triggered by confirmed clusters
- Aggressive caching — same tension doesn't need re-investigation daily
- Freshness checks are cheap — just hit the URL, check if content changed
- Synthesis is on-demand, not pre-computed

## Technology Directions

- **Graph database** (Neo4j or similar) as the primary store — this is a graph problem, not a relational problem
- **Vector embeddings** on every node for semantic search and deduplication
- **Lightweight agent orchestration** — not heavyweight workflows, but a job queue with priority and attention budget management
- **Server-side rendered public app** — fast, no JS required to see tensions. SEO-friendly so Google indexes tension pages
- **Streaming AI synthesis** — when you open a tension, the summary generates in real-time from the graph, not from a cached blob

## What This Is NOT

- Not a CMS where admins add orgs and sources
- Not a pipeline with fixed stages
- Not a feed reader that scrapes known URLs
- Not a matchmaker or recommendation engine
- Not a social network
- It's a **self-directed knowledge system** that explores civic reality autonomously, builds a graph, and lets humans navigate it

## Key Design Principles

- **Tension-driven, not entity-driven.** The AI goes looking for what matters, not what you told it to watch.
- **The city is the protagonist.** Not user preferences, not algorithmic engagement.
- **Don't overdefine.** Categories, scopes, response types — let them emerge from the graph.
- **Search engine, not platform.** Link out. Don't host. Don't matchmake.
- **Anyone can be a responder.** Orgs, grassroots, individuals. No hierarchy of legitimacy.
- **Autonomous but transparent.** The agents work on their own, but every node traces back to evidence.
- **Geographic fuzziness for safety.** Protect vulnerable people by blurring sensitive locations.
- **Two-tier surfacing.** Fast and labeled, or slow and verified. Never hidden.
- **Graph density as trust.** Not gatekeeping — transparency about what the graph knows.
- **Emergent topics.** The system detects tension agnostically. No predefined categories.

## Scenario: ICE Enforcement Activity

*Walkthrough of how the system handles a real, active civic crisis.*

**Sense:** Scouts pick up signal from multiple sources — local news reporting ICE arrests near a courthouse, social media posts of ICE vans in specific neighborhoods, community forums with sightings, an ACLU know-your-rights page going live, a GoFundMe for a detained person's legal defense, a church announcing an emergency sanctuary meeting. The cluster is detected: multiple signals, different sources, same geography, same timeframe.

**Understand:** Investigators trace the causal graph. What federal directive triggered this? Are local police cooperating? Which neighborhoods are affected? What's the history of enforcement in this area? Who are the actors — and who funds them? Federal contract databases reveal Enterprise Holdings provides vehicles, CoreCivic operates detention facilities, Palantir provides surveillance tech. Each becomes an Actor node with edges.

**Surface:** A user opens the app, sees a hot cluster on the map in Minneapolis. Taps it. Sees the tension, the context, and every response the system has found:
- Legal aid clinics and hotlines
- Know-your-rights trainings in Spanish, Somali, English
- GoFundMes for affected families (including an individual feeding 7 families — found via public post or human-reported)
- Sanctuary churches
- Protests and advocacy actions
- Boycott campaigns against corporate enablers (Enterprise, CoreCivic)
- Phone numbers for elected representatives
- Alternative companies to support

The individual feeding 7 families has the same standing as the ACLU. Both are Response nodes connected to the same Tension. The user sees both and decides how to plug in.

## Open Questions

- How does the system bootstrap in a new geography with no prior knowledge?
- What's the MVP that proves the core loop (sense → understand → surface) works?
- How does this relate to the existing Taproot infrastructure? Evolve it or start fresh?
- What's the right graph database choice given the existing Rust/PostgreSQL stack?
- How does the attention budget get initially seeded? Population density? News volume? User requests?

## Next Steps

→ Resolve open questions
→ `/workflows:plan` when ready for implementation
