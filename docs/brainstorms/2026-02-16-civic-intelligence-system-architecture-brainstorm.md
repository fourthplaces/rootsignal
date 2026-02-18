---
date: 2026-02-16
topic: civic-intelligence-system-architecture
supersedes: civic-tension-search-engine-architecture
---

# Root Signal — Architecture from Scratch

## What This Is

A self-directed knowledge system that continuously explores civic reality across the web, builds a living graph of everything civic in a place, and lets humans navigate it. A search engine for civic life.

**Tension is one layer — the most urgent layer — but the system maps all of civic reality:** community needs, ecological stewardship, civic engagement, ethical consumption, and everything in between. The graph is the whole picture of civic life in a place. Tension is what makes it pulse and shift.

## Core Problem

The web is full of civic signal buried in noise. Protests, food drives, GoFundMes, solidarity events, boycotts, mutual aid, environmental disasters, policy changes, volunteer opportunities, local co-ops, tool libraries, rain garden programs, city council votes, neighborhood cleanups — all scattered across 50 platforms and buried in algorithmic feeds that don't care about civic life. No single place raises this signal, organizes it, and helps people plug in.

## Fundamental Insight

This is a **knowledge graph that builds itself.** The AI isn't extracting data into tables — it's constructing a living model of civic reality. Every tension, need, resource, organization, event, place, and person is a node connected to everything else. The system's job is to grow that graph continuously and autonomously.

## Six Domains of Civic Life

The graph doesn't enforce these categories — they emerge naturally. But these are the domains the system needs to cover:

### 1. Human Community Needs
Mutual aid, volunteering, support networks, meal trains, clothing drives, tutoring, flood relief, peer support, neighborhood safety.

*"Where can I volunteer this weekend?" · "My neighbor just had surgery — is there a meal train?" · "I'm a nurse — are there free clinics that need weekend volunteers?" · "I only have 2 hours a week — what can I realistically do?"*

### 2. Ecological Stewardship
Invasive species removal, river cleanups, tree planting, water quality monitoring, wildlife rescue, native plant restoration, citizen science, prescribed burns.

*"Where are invasive species removal events near me?" · "There's a bunch of dead fish in Lake Nokomis — who do I tell?" · "Are there citizen science projects tracking pollinators here?"*

### 3. Civic Engagement
City council, public hearings, elections, boards and commissions, policy advocacy, accountability, budget transparency, zoning, school boards.

*"How do I testify at a public hearing about the new zoning proposal?" · "How did my council member vote on rent stabilization?" · "I want to run for school board — where do I start?"*

### 4. Ethical Consumption
Local food, co-ops, fair wage businesses, repair shops, tool libraries, zero-waste options, buy-nothing groups, CSAs, Black-owned businesses, alternatives to Amazon.

*"I want to stop buying from Amazon — what are my local alternatives?" · "Where can I get my shoes repaired?" · "Is there a tool library near me?"*

### 5. Cross-Domain (Where the real value lives)
Real people don't think in categories. The most important queries span multiple domains.

*"I want to make a difference in my neighborhood — where do I start?" · "My block has a vacant lot — what are the options for it?" · "Give me everything happening near 55408 this week"*

### 6. Tension (The urgent pulse)
When civic reality is under stress — ICE raids, environmental disasters, housing crises, school closures, police accountability — the graph lights up. Tension is not a separate domain. It's a state that any part of the graph can enter. Tension is what turns a food shelf from a resource into an urgent need, a city council meeting from routine into critical.

## Three Loops

### Loop 1: Sense

The system is always listening. Not scraping a fixed list of sources — *searching*. Given a geographic area, the AI asks: "what is civic life like here right now?" It searches the open web, social platforms, government data, fundraising sites, event platforms, org websites, business directories — everything. It doesn't wait for a human to add sources. It hunts.

This is closer to a search engine crawler than a feed reader. Breadth-first exploration of civic reality, guided by geography and recency.

### Loop 2: Understand

When the system finds signal, it builds context. For a tension: why is it happening, who's involved, what caused it, who's responding. For a resource: what is it, who runs it, when is it available, is it current. For an event: what's it about, who's behind it, what's the civic context. For an actor: what do they do, who funds them, what are they connected to.

The output isn't a flat record. It's a **graph node with edges** — connected to everything else the system knows.

**"Follow the money" is a core investigation pattern.** The investigator traces funding and enablement chains — who has the federal contracts? Who provides the technology? Who operates the facilities? Who profits? These become Actor nodes with `funds`, `contracts_with`, `enables` edges. Crucially, these actors are shared across geographies — Enterprise's contract with ICE connects to tensions in every city, not just one.

### Loop 3: Surface

A human points at a place on the map — or types a question — and the system shows them the civic graph for that query. Not a list of blue links. A living, structured picture of civic reality.

Three views — feed, map, summary — all projections of the same underlying graph.

## Data Model: A Graph, Not Tables

The core data structure is a knowledge graph:

```
Node: Tension (a civic stress point — urgent, temporal)
Node: Need (something a community lacks)
Node: Response (an action taken in response to a tension or need — GoFundMe, boycott, food drive, legal clinic, protest)
Node: Resource (something that exists to help — food shelf, tool library, clinic)
Node: Event (something happening — cleanup, meeting, protest, training)
Node: Actor (org, person, government body, company, grassroots group)
Node: Place (neighborhood, city, region, park, watershed, ward, block)
Node: Policy (law, ordinance, executive order, budget line item)
Node: Evidence (article, post, filing, dataset, GoFundMe, event listing)

Edge: caused_by, responds_to, located_in, connected_to,
      sourced_from, affects, organized_by, funds,
      contracts_with, enables, serves, advocates_for,
      provides, part_of
```

**Note:** Response vs Resource vs Event: A Response is specifically linked to a Tension or Need via a `responds_to` edge — it is civic action taken in reaction to something. A Resource is standing civic infrastructure (a food shelf exists whether or not there's a crisis). An Event may or may not be a Response depending on context (a weekly cleanup is a Resource; an emergency cleanup after a disaster is a Response). The graph captures this through edges, not rigid categories.

Everything is a node. Everything is connected. The AI continuously adds nodes and edges. A user query is a graph traversal.

**Tensions share actors across geographies.** The corporate funding graph (Enterprise, Palantir, CoreCivic) connects to tensions in every city where those actors operate.

**Resources, events, and needs are local.** The tool library on your block, the food shelf on Lake Street, the cleanup in your park — these are hyperlocal nodes.

**The graph has both the macro and the micro.** Federal policy connects to neighborhood impact. A zip code query pulls both.

## Signal Sources

Three classes of signal, all first-class:

1. **Structured sources** — government databases (USAspending, FPDS, 311, election data), city council agendas/minutes/votes, permit filings, court records, business registrations, park jurisdiction data, ward maps
2. **Unstructured/web sources** — news, social media, GoFundMe, event platforms, org websites, community forums, business directories, co-op listings, Yelp/Google reviews (for civic-relevant businesses), neighborhood association sites
3. **Human-reported** — people submit signal directly ("my sister is feeding 7 families" · "the food shelf on Lake Street just ran out of diapers" · "there's a new buy-nothing group for Longfellow")

Human-reported signal enters the graph with the same standing as AI-discovered signal. **No hierarchy of legitimacy.**

**Source acquisition policy:** "Everything" ingestion has legal and compliance boundaries.

- **Public government data** (USAspending, FPDS, 311, council minutes, election data): Public record. No restrictions. Primary source class.
- **News articles:** Fair use for extraction of facts (not full-text reproduction). Link out to originals. Never host full article text.
- **Social media:** Use official APIs where available (rate limits apply). For platforms without APIs, scrape only public posts. Never scrape private/friends-only content. Respect robots.txt.
- **GoFundMe, event platforms, org websites:** Public web pages. Extract structured data (title, description, link, date, location). Link back to originals. Never host hosted content.
- **Review platforms (Yelp, Google):** Higher legal risk. Extract only civic-relevant facts (e.g., "this business pays a living wage" from a review). Do NOT reproduce reviews or ratings.
- **Redistribution:** The system does not redistribute source content. It extracts facts, creates graph nodes, and links back to originals. The graph is derived knowledge, not a content mirror.
- **Opt-out:** Any source (individual, org, platform) can request removal of their data from the graph. This must be supported from day one.

## AI Architecture: A Swarm of Specialized Agents

Not one big pipeline. Autonomous agents with distinct roles:

- **Scout agents** — continuously searching the web for civic signal in target geographies. Cheap, fast, broad. Running constantly. Not just tension — everything civic.
- **Investigator agents** — when a scout finds something significant, an investigator goes deep. Follows the causal chain, the money, the policy trail. Builds the graph edges. Slower, more expensive, thorough.
- **Response discovery agents** — given a tension or need, find everyone responding to it. Scour GoFundMe, event platforms, org websites, social media. Individuals, grassroots, orgs — all of it.
- **Resource mapping agents** — build the baseline graph of civic infrastructure in a geography. Food shelves, tool libraries, co-ops, repair shops, clinics, parks, neighborhood associations, ward maps. This is the "steady state" graph that tension activates.
- **Synthesis agents** — generate human-readable summaries, briefings, map annotations from the raw graph. Run on-demand when a user looks at something or asks a question.
- **Freshness agents** — revisit existing nodes. Is this resource still open? Did this event happen? Is this GoFundMe still active? Is the food shelf still accepting donations? The graph decays without maintenance. **Freshness matters most for grassroots responses and time-sensitive resources.**
- **Query agents** — handle natural language questions from users. Parse intent, traverse the graph, synthesize an answer. "I'm a nurse — are there free clinics that need weekend volunteers?" becomes a graph query: find Resource nodes (type: clinic, cost: free) with Need edges (type: volunteer, skill: nursing, time: weekend) near user's location.

These agents are autonomous. The system has an **attention budget** — compute allocated across geographies based on activity level. Hot areas get more scouts. Active tensions get more investigators. Quiet areas still get resource mapping agents maintaining the baseline.

## Interface

Dead simple. A map. A search bar.

- Zoom into anywhere. See civic activity on the map — tension clusters burn hot, resources and events are steady markers.
- Tap anything. See the node, its context, its connections.
- Search naturally: "What can I do this weekend near 55408?" or "Who's working on affordable housing in Saint Paul?" or "I have a truck and a free Saturday" — and get a structured result from the graph.
- Every actionable node has a link out: signup, donation, event page, phone number, address. The system doesn't host these — it's a search engine, not a platform.

### Handling Query Types

**Direct queries** ("Where can I volunteer this weekend?") → Graph traversal: Event nodes with volunteer Need edges, near user, this weekend.

**Situational queries** ("I just got laid off — what resources are available?") → Graph traversal: Resource nodes tagged employment/benefits/support near user's county. Synthesis agent wraps it in a human answer.

**Skill-based queries** ("I'm an electrician — who needs free electrical work?") → Graph traversal: Need nodes matching skill, connected to nonprofit/community Actor nodes, near user.

**Ambient queries** ("What's happening in Frogtown right now?") → Graph traversal: all active nodes in Place(Frogtown), sorted by recency and significance. Feed + map + summary.

**Tension queries** ("What's going on with ICE in Minneapolis?") → Deep graph traversal: Tension node with all edges — causes, actors, funding, responses, evidence, timeline.

**Cross-domain queries** ("I want to make a difference in my neighborhood — where do I start?") → The hardest and most valuable. Synthesis agent surveys the full graph around user's location, identifies the highest-need and lowest-barrier entry points, and presents a curated starting point across all domains.

**Underspecified queries** ("Help" · "What's going on?") → The system asks clarifying questions: "Where are you? What are you looking for?" Or it defaults to: here's the civic pulse of your area right now.

## Pressure Testing: Resolved Questions

### Safety: Protecting vulnerable people

The system maps tension geographically — but that geographic data can be weaponized. A real-time map of where people are seeking help from ICE raids is a surveillance tool that cuts both ways.

**Resolution: Geographic fuzziness as a core feature.** The public-facing graph never surfaces exact locations for sensitive tensions. "South Minneapolis" yes. Specific intersection, no. The system knows precise data internally but the public view is deliberately blurred. The level of blur scales with the sensitivity of the tension.

**Sensitive-signal holdback rules.** Not all tension signals should surface at Tier 1 speed. Signals classified as high-risk (enforcement activity, immigration raids, location of vulnerable populations) have explicit holdback rules before map visibility:

1. **Corroboration threshold for sensitive classes.** A single unverified report of ICE activity does NOT go on the map. It requires corroboration from at least one independent source (news outlet, second reporter, org confirmation) OR explicit human-reported confirmation from a trusted source.
2. **Sensitivity classification.** The system maintains a sensitivity model (not predefined categories, but learned patterns) that recognizes when a signal could endanger people if surfaced prematurely or falsely. Enforcement activity, location of undocumented people, domestic violence shelters — these trigger holdback.
3. **False report risk.** Adversarial false reports of enforcement activity can cause panic. The corroboration threshold is the defense — but the system also tracks false-report patterns and escalates to human review when a pattern is detected.

**Identity safety is critical.** When someone asks "I'm undocumented — what's safe for me to participate in?" the system must surface only resources explicitly designed for that context, and must never create a data trail that could be used against the person asking.

### Privacy: Concrete data-handling model

The system promises "never create a data trail" while supporting location- and identity-sensitive queries in a public no-auth app. These are in tension. Concrete policies:

**Query privacy:**
- **No query logging with IP or device identifiers.** Queries are processed, answered, and discarded. The system does not build user profiles.
- **No auth required.** The public interface is fully anonymous. No accounts, no cookies that track across sessions.
- **Sensitive query stripping.** If a query contains identity markers ("I'm undocumented," "I'm in recovery"), the synthesis agent uses the context to improve the answer but the raw query is NOT stored, logged, or used for analytics.
- **No analytics that could reconstruct user behavior.** Aggregate metrics only (total queries per day, most-viewed tensions). Never per-user or per-session.

**Data retention:**
- **Graph nodes (civic data):** Retained indefinitely, subject to freshness decay. This is public information organized, not private data.
- **Human-reported signal:** The content enters the graph (the node). The identity of the reporter is NOT stored unless they explicitly choose to be credited. Submission is anonymous by default.
- **Server logs:** Minimal. No IP logging on query endpoints. Infrastructure logs (error tracking, performance) retain no query content and auto-expire after 30 days.

**Threat model:**
- Assume the database could be subpoenaed. Store nothing that could be used to identify who asked what.
- Assume the public interface will be accessed by adversarial actors (ICE, bad-faith reporters). The holdback rules and geographic fuzziness are the defense.

### Speed vs. Truth: Two-tier surfacing

Civic emergencies happen NOW. Verification takes time.

**Resolution: Two tiers.**

- **Tier 1: Unverified but sourced** — surfaces fast (minutes). Every node shows its source. The user sees the raw signal and judges for themselves.
- **Tier 2: Investigated** — surfaces slower (hours). Cross-referenced, contextualized, evidence-linked.

The system doesn't hide unverified signal — it labels it. Speed and truth coexist through transparency about confidence.

### Trust and Ordering: Honest about ranking

The system claims "no ranking, no gatekeeping" — but any system that decides what to show first is ranking. Be honest about this.

**Resolution: The system ranks. It does so transparently and by explicit, visible criteria.**

What the system does NOT do: editorially decide that one response is "better" than another, or suppress results it disagrees with.

What the system DOES do (and should be transparent about):
- **Orders by graph density.** Nodes with more corroborating edges surface higher. A response confirmed by 3 independent sources appears before one with no corroboration. This is ranking by evidence, and the user can see why — the evidence is visible.
- **Orders by freshness.** Recently verified nodes surface higher than stale ones. Visible via `last_verified` timestamp.
- **Orders by proximity.** Closer results surface higher for location-based queries. Obvious and expected.
- **Source reputation influences visibility.** Sources that consistently produce corroborated signal have their outputs surface faster. This is ranking by track record. The system should be transparent that this happens.

**The key commitment:** Every ranking factor is visible to the user. No black-box algorithm. If a result is lower, the user can see why (thin graph, stale, far away, uncorroborated). The system never hides results — it orders them.

### Editorial Stance: Emergent topics, not predefined categories

**Resolution: The system doesn't predefine topics.** It detects civic activity agnostically. Topics emerge descriptively from the graph. The system's editorial stance: **"here is civic reality, here's everything connected to it."** Not what's right or wrong.

### Adversarial Inputs: The graph is its own defense

**Resolution: Three layers.**

1. **Signal corroboration.** Isolated reports stay thin in the graph. Corroborated signal grows edges.
2. **Source reputation over time.** Sources that produce signal that gets corroborated naturally gain influence. Sources that don't, fade.
3. **Rate limiting and anomaly detection.** Suspicious patterns get flagged, not surfaced.

**The graph itself is the defense.** Fake signal doesn't grow edges. Real signal does.

### Freshness & Timeliness

The graph decays. Events pass. GoFundMes close. Food shelves run out. Volunteer slots fill.

**Resolution: Freshness is a first-class property of every node.** Every node has a `last_verified` timestamp and a `freshness_priority` based on its type and volatility. Freshness agents prioritize:
1. Time-sensitive resources (events, drives, emergency needs) — check daily or more
2. Grassroots/individual responses — check frequently (they change fast)
3. Stable infrastructure (co-ops, tool libraries, parks) — check weekly/monthly
4. Policy/actor nodes — check on trigger (new vote, new filing)

When a user asks "Is the food shelf on Lake Street still open?" the system either knows (recently checked) or triggers an immediate freshness check.

### Hyperlocal Precision

Real queries are hyperlocal: "What's happening on my block?" · "Anything within walking distance of 38th and Chicago?"

**Resolution: The Place graph must go deep.** Not just cities and neighborhoods — wards, blocks, parks, intersections, watersheds, school districts, library branches. Every resource and event is placed precisely. Geo-to-civic mapping (zip → ward → council member → district) is a core capability.

### Identity & Inclusion

"I'm undocumented — what's safe?" · "Are there LGBTQ-friendly orgs?" · "I use a wheelchair — which events are accessible?" · "Is there anything in Hmong?"

**Resolution: These are graph properties, not filters.** Accessibility, language, safety profile, age-appropriateness — these are edges on Resource/Event nodes. The system surfaces them when relevant to the query. It doesn't ask people to self-identify into categories — it surfaces information that is relevant to the context of the question.

Language support is especially critical. The graph should contain resources in the languages spoken in the community (for Twin Cities: English, Spanish, Somali, Hmong, Karen, Oromo, and more).

### Emotional State & Motivation

"I feel helpless about climate change — what can I actually do?" · "I'm burned out on volunteering — something low-effort?" · "Everything feels hopeless — is anything actually working?"

**Resolution: The synthesis agent handles this.** The graph has all the raw data. The synthesis agent, when generating an answer to a query with emotional context, can:
- Prioritize high-efficacy actions ("here's something where your contribution visibly matters")
- Surface low-barrier entry points ("this takes 15 minutes from your couch")
- Include evidence of impact ("this org helped 200 families last month")

The graph doesn't store emotion. The synthesis layer responds to it.

### Scope Boundaries

"What's a good restaurant near me?" · "Can you help me file my taxes?" · "What's the weather?"

**Resolution: The system has a civic lens.** A restaurant recommendation is out of scope unless there's a civic angle (fair wage, local, Black-owned). Tax help is in scope if there's a community resource for it (free tax prep at the library). The boundary is: **does this connect to the civic graph?** If it has nodes and edges in the graph, it's in scope. If not, the system says so honestly: "That's outside what I track, but here's where you might look."

### Data Integrity

"Two different sources say different times for the same meeting" · "This was posted six months ago — is it still relevant?"

**Resolution: Deduplication by graph identity, conflict resolution by multiple factors.** When two Evidence nodes point to the same Event but disagree, resolution uses (in priority order):

1. **Source reliability** — an org's official website outweighs a third-party listing
2. **Corroboration count** — the version confirmed by more independent sources wins
3. **Freshness** — more recently verified version preferred, all else equal
4. **Conflict state** — if resolution is ambiguous, the system shows BOTH versions with a visible conflict flag: "Two sources disagree on the time for this event. [Source A] says 6pm (verified today). [Source B] says 7pm (verified 3 days ago)."

Staleness is always visible: "This listing is 6 months old and hasn't been verified recently."

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
| Resource mapping agents | $5-50 | Baseline graph maintenance |
| Freshness agents | $5-50 | URL checks + verification |
| Query/synthesis agents | $5-100 | On-demand, scales with users |
| Embeddings + graph ops | $1-5 | Pennies per embedding |
| Graph DB hosting | $3-17 | $100-500/month |
| **Total per metro** | **$99-992/day** | **$3,000-30,000/month** |

**Cost sensitivity by scenario (single metro):**

| Scenario | Monthly Cost | Key Driver |
|----------|-------------|------------|
| **Phase 1a MVP** (1 tension, proving the loop) | **$500-1,500** | Minimal scout volume, few investigations |
| **Phase 1b** (10 active tensions, full system) | **$3,000-7,000** | Moderate scout volume, regular investigations |
| **Full domain coverage** (all 6 civic domains) | **$7,000-15,000** | High scout volume, resource mapping agents |
| **High-activity crisis period** (e.g., major ICE operations) | **$15,000-30,000** | Investigation and response discovery spike |
| **National coverage** | **$30,000-70,000** | Attention-budget-driven, scales with active metros |

**What drives cost up:** User volume (more synthesis queries), source diversity (more platforms to scrape), investigation depth (more multi-turn agent runs), crisis periods (more active tensions requiring frequent freshness checks). Social media API costs are the wildcard — platform pricing changes can 2-3x the monitoring line item overnight.

**What drives cost down:** Aggressive caching, Haiku-class models for scouts, scheduled (not continuous) resource mapping, on-demand synthesis, HTTP HEAD requests for freshness checks.

**Cost management:**
- Scouts use cheap, fast models (Haiku-class)
- Investigators use expensive models only when triggered
- Resource mapping agents run on schedule, not constantly
- Aggressive caching
- Freshness checks are cheap (HTTP head requests + change detection)
- Synthesis is on-demand, not pre-computed

## Technology Decisions

### Graph Database: Neo4j (via Aura)

PostgreSQL was fighting us on graph operations from day one (clustering, multi-hop traversals, community detection). Neo4j gives us natively:

- **Cypher queries** for multi-hop traversals ("trace ICE funding through 5 hops of corporate relationships")
- **Graph Data Science (GDS) library** — community detection (Louvain/Leiden), centrality (PageRank, Betweenness), link prediction, node embeddings. Replaces hand-rolled clustering.
- **Native spatial** — POINT type, haversine distance, bounding box queries. "Find everything within 5 miles" is built in.
- **Native vector search** — cosine/euclidean similarity on embeddings. Replaces pgvector.
- **Native full-text search** — Lucene-based. Replaces tsvector.
- **Deployment:** Neo4j Aura Professional (~$65+/month). Free tier for prototyping.

### Language: Rust (with escape hatches)

Rust stays for the compute-heavy layer where it earns its keep on cost and performance:

- Web crawlers and scrapers
- AI agent orchestration (scout, investigator, response discovery, freshness agents)
- Signal extraction and embedding generation
- Attention budget management

### Rust + Neo4j Driver: neo4rs (with eyes open)

The `neo4rs` crate (neo4j-labs) is the Rust driver. It works but has known risks:

**Known issues:**
- Pre-1.0 (0.9.0 has been in RC for 15 months)
- RowStream can return fewer results than expected in some cases (#281)
- Mutations silently fail if you don't consume the result stream (#112) — every write must iterate results
- Connection pool can leak under high concurrency (#106)
- No native VECTOR type support yet (use LIST<FLOAT> instead)
- No element ID support (Neo4j 5.x feature, #97)
- Bus factor of ~1.5 maintainers

**Mitigations:**
- Always consume result streams on writes (wrap in helper)
- Validate result counts for critical queries
- Monitor connection pool health
- Use LIST<FLOAT> for embeddings until VECTOR type lands

**Escape hatches if neo4rs becomes a blocker:**
1. **Neo4j HTTP API directly** — bypass driver, hit REST endpoints with raw Cypher. Full control, no driver dependency.
2. **Thin TypeScript service** — official Neo4j TypeScript driver is battle-tested. Rust services talk to it over gRPC/HTTP for graph operations.
3. **Hybrid** — use neo4rs for reads (lower risk), HTTP API for writes (avoid the mutation footgun).

Decision: start with neo4rs, build defensive wrappers, re-evaluate when/if it becomes an issue. Multiple clean migration paths exist.

### Other Technology

- **Vector embeddings** on every node for semantic search and deduplication (stored as LIST<FLOAT> in Neo4j until native VECTOR support lands in neo4rs)
- **Lightweight agent orchestration** — Restate for durable workflows (already in use), with priority and attention budget management
- **Server-side rendered public app** — fast, no JS required to see the graph. SEO-friendly so Google indexes civic pages
- **Streaming AI synthesis** — answers and summaries generate in real-time from the graph
- **Geo-to-civic mapping** — zip → ward → council member → district → jurisdiction as a core service

## Existing Building Blocks

The old system (`rootsignal-core`, `rootsignal-domains`, `rootsignal-server`, `admin-app`) has been archived. This is a clean start. Two utility crates survive and are directly useful:

- **`ai-client`** — Provider-agnostic LLM client supporting Claude, OpenAI, and OpenRouter. Includes tool use (multi-turn), structured output extraction, and embedding generation. Every agent type (scout, investigator, synthesis, query) builds on this.
- **`apify-client`** — Apify REST client for scraping Instagram, Facebook, X/Twitter, TikTok, and GoFundMe. Ready-made scraping for five of the most important unstructured signal sources. Scout and response discovery agents use this directly.
- **`twilio-rs`** — OTP verification and WebRTC TURN/STUN. Not needed for the no-auth public interface, but available if identity verification is ever needed (e.g., trusted human reporters).

Everything else — graph layer, agent orchestration, public API, web interface — is built from scratch.

## What This Is NOT

- Not a CMS where admins add orgs and sources
- Not a pipeline with fixed stages
- Not a feed reader that scrapes known URLs
- Not a matchmaker or recommendation engine
- Not a social network
- Not Yelp for civic life
- It's a **self-directed knowledge system** that explores civic reality autonomously, builds a graph, and lets humans navigate it

## Key Design Principles

- **Civic reality is the product.** Not tension alone — the full picture of civic life in a place.
- **Tension is a state, not a category.** Any part of the graph can enter tension. That's what makes it pulse.
- **The city is the protagonist.** Not user preferences, not algorithmic engagement.
- **Don't overdefine.** Domains, categories, response types — let them emerge from the graph.
- **Search engine, not platform.** Link out. Don't host. Don't matchmake.
- **Anyone can be a responder.** Orgs, grassroots, individuals. No hierarchy of legitimacy.
- **Autonomous but transparent.** The agents work on their own, but every node traces back to evidence.
- **Geographic fuzziness for safety.** Protect vulnerable people by blurring sensitive locations.
- **Two-tier surfacing.** Fast and labeled, or slow and verified. Never hidden.
- **Transparent ranking.** The system ranks by graph density, freshness, and proximity — and every ranking factor is visible to the user. No black box.
- **Emergent topics.** The system detects activity agnostically. No predefined taxonomy.
- **Freshness is a first-class property.** The graph decays. Maintenance is not optional.
- **Identity safety by design.** Never create data trails that could be used against the people the system serves.
- **The civic lens.** Everything in scope connects to the graph. Everything else, the system honestly defers.

## Scenario: ICE Enforcement Activity

*Walkthrough of how the system handles a real, active civic crisis.*

**Sense:** Scouts pick up signal from multiple sources — local news reporting ICE arrests near a courthouse, social media posts of ICE vans in specific neighborhoods, community forums with sightings, an ACLU know-your-rights page going live, a GoFundMe for a detained person's legal defense, a church announcing an emergency sanctuary meeting. The cluster is detected: multiple signals, different sources, same geography, same timeframe.

**Understand:** Investigators trace the causal graph. What federal directive triggered this? Are local police cooperating? Which neighborhoods are affected? What's the history? Who funds and enables ICE operations? Federal contract databases reveal Enterprise Holdings provides vehicles, CoreCivic operates detention facilities, Palantir provides surveillance tech, Thomson Reuters and LexisNexis sell personal data. Each becomes an Actor node with edges — shared across every city where they operate.

**Surface:** A user opens the app, sees a hot cluster on the map in Minneapolis. Taps it. Sees the tension, the context, and every response:
- Legal aid clinics and hotlines
- Know-your-rights trainings in Spanish, Somali, English
- GoFundMes for affected families (including an individual feeding 7 families — found via public post or human-reported)
- Sanctuary churches
- Protests and advocacy actions
- Boycott campaigns against corporate enablers (Enterprise, CoreCivic)
- Phone numbers for elected representatives
- Alternative companies to support

The individual feeding 7 families has the same standing as the ACLU. Both are Response nodes connected to the same Tension.

## Scenario: "I want to make a difference in my neighborhood — where do I start?"

*Walkthrough of a cross-domain ambient query.*

**Query agent** parses: location = user's neighborhood, intent = civic engagement, specificity = low, motivation = high, knowledge = low. This is a "show me everything" query.

**Graph traversal:** Pull all active nodes within the user's neighborhood — upcoming events, active needs, resources, tensions, organizations.

**Synthesis agent** surveys the results and generates a starting point:

> **Here's what's happening in Longfellow this week:**
>
> **Urgent:** The food shelf on Minnehaha Ave is critically low on diapers and formula. [Link to donate / volunteer]
>
> **This weekend:** Invasive buckthorn removal at Minnehaha Falls — Saturday 9am, no experience needed. [Link]
>
> **Ongoing need:** Whittier Elementary needs reading tutors, Tuesday and Thursday mornings. [Link]
>
> **Civic:** The city is accepting public comments on the Hiawatha Ave redesign through March 1. [Link to comment portal]
>
> **Shop local:** Longfellow has 3 co-ops, a tool library, and a bike repair co-op. [Links]
>
> **Your neighborhood association** meets the second Thursday of every month. Next meeting: Feb 20. [Link]

No categories imposed. No ranking. Just: here's civic reality around you, organized by urgency and type. The user finds their own entry point.

## Phasing: Tension First

Tension is the sharpest wedge. It's urgent, it's happening right now, and it's the hardest thing to find on your own. Start here. Prove the core loop. Everything else layers on top of the same infrastructure.

### Phase 1: Tension (NOW)

**Goal:** Detect tension in one metro (Twin Cities), investigate it, surface all responses, let people plug in.

#### Phase 1a: Smallest Loop (the literal MVP)

The narrowest possible proof that the core loop works. One tension, end-to-end.

**What gets built:**
- Neo4j graph infrastructure with core node/edge types
- One scout agent scanning web + news for tension signals in Twin Cities
- One investigator agent that traces the causal graph for a detected tension
- One response discovery agent that finds who's responding
- A minimal public interface: map with tension markers + tension detail page showing the graph
- Basic geographic queries (find tensions near a point)

**What this does NOT include yet:** Human-reported signal, freshness agents, two-tier surfacing, geographic fuzziness, feed view, AI summary, search bar, attention budget. These come in 1b.

**Success metric:** A real person in the Twin Cities can open the app, see a tension they didn't know about, understand why it's happening, and find at least one way to plug in. If that works once, the loop is proven.

#### Phase 1b: Full tension system

Layer on the rest:
- Freshness agents keeping time-sensitive responses current
- Human-reported signal input
- Geographic fuzziness for sensitive tensions
- Two-tier surfacing (fast/unverified + slow/investigated)
- Sensitive-signal holdback rules
- Full public interface (map + search bar + feed + summary)
- Privacy-preserving query handling (no logging, no tracking)
- Multiple scout agents with attention budget

**Success metric:** 10 active tensions in the Twin Cities, each with investigated causal graphs and discovered responses. Users can search by location and browse tensions. Sensitive tensions are fuzzed. The graph stays fresh.

**Built greenfield** on the existing utility crates (`ai-client`, `apify-client`) but otherwise a clean start — new crate structure, new data layer (Neo4j), new agent orchestration.

### Phase 2: Community Needs + Civic Engagement

**Goal:** Expand the graph beyond tension into steady-state civic life.

**What gets added:**
- Resource mapping agents building the baseline civic graph (food shelves, clinics, shelters, neighborhood associations, ward maps)
- Scout agents expanding to community needs (volunteer opportunities, mutual aid, support networks)
- Scout agents expanding to civic engagement (council meetings, public hearings, elections, policy advocacy)
- Query agents handling natural language questions across all domains
- Geo-to-civic mapping (zip → ward → council member → district)

**What this proves:** The system works for both urgent and ambient civic queries. Not just "what's the crisis" but "how do I get involved."

### Phase 3: Ecological Stewardship + Ethical Consumption

**Goal:** Full civic intelligence — the complete picture of civic life in a place.

**What gets added:**
- Scout agents for ecological signal (invasive species, water quality, restoration events, wildlife, citizen science)
- Scout agents for ethical consumption (co-ops, repair shops, tool libraries, CSAs, fair-wage businesses, buy-nothing groups)
- Deeper cross-domain synthesis (connecting the vacant lot to the zoning policy to the community garden to the neighborhood association)

**What this proves:** The system is a civic search engine, not just a crisis tracker.

### Phase 4: National Scale

**Goal:** Expand beyond Twin Cities. Attention-budget-driven national coverage.

**What gets added:**
- Autonomous geographic expansion — when tension is detected in a new area, scouts spin up
- City-agnostic civic mapping (different jurisdictions, different data sources, same graph structure)
- Cross-geography actor connections (Enterprise contract with ICE is the same node in every city)
- User-requested geography ("show me what's happening in Portland")

### What stays constant across all phases

- The graph data model
- The agent architecture (scout → investigate → respond → synthesize → freshen)
- The interface (map + search + feed + summary)
- The design principles (no hierarchy, emergent topics, graph density as trust, geographic fuzziness)
- The public-facing, no-auth, civic search engine UX

Each phase turns on more agent types and expands what the scouts look for. The architecture doesn't change — the scope does.

## Open Questions

- What's the Neo4j schema for Phase 1a? What node types, edge types, indexes, and constraints are needed to support the core loop?
- What's the greenfield crate structure? How do agents, graph operations, and the public API decompose into modules?
- How does the attention budget get initially seeded? Start with known tension points? News volume? Manual seed list?
- What's the sustainability/funding model? Grant-funded? Municipal contracts? Nonprofit?
- What's the sensitivity classification model for holdback rules? Learned from data, manually seeded, or hybrid?
- How do we handle opt-out requests at scale? (Individual nodes, entire source domains, specific actors requesting removal)

## Competitive Landscape

Nothing does the full loop. Existing tools each do one slice:

| What it does | Who does it | What's missing |
|---|---|---|
| Track conflict/tension data | ACLED | No action, no responses, not for regular people |
| Find events to attend | Mobilize, Find My Protest | No civic context, org-driven only |
| Mutual aid coordination | Various local networks | Hyperlocal, not discoverable, not connected |
| Civic deliberation | Collective Intelligence Project | Academic, narrow scope |
| News aggregation | Google News, Apple News | No civic lens, no structure, no action |

**Nobody is building the civic knowledge graph.** Nobody is connecting the GoFundMe to the boycott to the legal clinic to the federal policy to the tool library to the neighborhood meeting. That's the gap.

## Next Steps

→ Resolve open questions
→ `/workflows:plan` when ready for implementation
