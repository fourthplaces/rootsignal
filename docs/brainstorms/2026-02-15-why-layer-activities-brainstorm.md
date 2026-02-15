---
date: 2026-02-15
topic: why-layer-activities
---

# The "Why" Layer: Activities

## The Idea

Signals tell you **what** is being broadcast. Activities tell you **why**.

A signal is a broadcast — someone put it into the world. "Church offering rent relief." "Free legal clinic March 5." "River cleanup Saturday." The system collects these and makes them findable. But signals are symptoms. The "why" layer asks: what's happening underneath that's producing these symptoms?

The answer is an **Activity** — an evidence-backed explanation for why a cluster of signals exists.

## The Example

Grace Church posts on Instagram: "We're offering rent relief for families who are afraid to leave their homes to work."

The signal extraction layer captures this as a `give`: rent relief available. But the content hints at something deeper — "afraid to leave their homes to work" is not normal food-pantry language. The system flags this for investigation.

The investigation layer crawls Grace Church's website, their Facebook page, linked articles. It finds:

- Grace Church Facebook: "We're seeing families who can't leave their homes to work"
- ACLU Minnesota website: "Know your rights if stopped by immigration agents"
- Sahan Journal (linked from an org's page): "ICE arrests reported in Cedar-Riverside neighborhood"
- USAspending (already in the system): "$50M ICE contract awarded to GEO Group"

The LLM synthesizes this into an Activity:

```
Activity: "ICE conducting street-level enforcement operations in Twin Cities"

Evidence:
- Grace Church Facebook: "families are afraid to leave their homes"
- ACLU MN website: "know your rights if stopped by immigration agents"
- Sahan Journal: "ICE arrests reported in Cedar-Riverside"
- USAspending: "$50M ICE contract awarded to GEO Group"

Signals linked:
- Grace Church: rent relief (give)
- St. Mary's: grocery delivery (give)
- ACLU MN: know-your-rights workshop (event)
- Mutual aid network: ride sharing (give)
```

Four different organizations broadcasting four different things. One underlying reason. The Activity is the convergence point.

## How It Works

### Detection: Content Hints at Something Deeper

The LLM is already reading content during signal extraction. Some content is straightforward — "food pantry open Mon-Fri 9-5" is just a `give`. But some content carries weight beyond the surface broadcast:

- Unusual language: "afraid to leave their homes"
- Causal framing: "because of recent enforcement actions"
- Emergency tone from an entity that doesn't normally signal urgency
- A type of offering that's outside the entity's normal pattern (a church doing rent relief)

The LLM detects this naturally during extraction. No separate anomaly detection layer needed. When the content hints at something deeper, the signal gets flagged for "why" investigation.

### Investigation: Three Evidence Paths

The investigation layer gathers evidence through three paths, all producing snapshotted, citable content:

#### Path 1: Follow the Link Trail

The page snapshot that triggered the investigation already contains links — to news articles, partner organizations, government pages. These links are the trail:

1. **Extract links from the triggering snapshot** — the page content has embedded URLs
2. **LLM decides which links are relevant** — a linked news article about community conditions = follow. A donate button or nav link = skip.
3. **Crawl the relevant links** — fetch those pages, snapshot them (now they're auditable evidence)
4. **Repeat** — the newly fetched pages have their own links. The LLM decides whether to follow deeper.

The depth is bounded (max 3 hops) but the LLM can stop earlier. Every page fetched gets snapshotted — the entire trail is auditable.

#### Path 2: Search for Corroboration

When the link trail is thin or absent, the investigation layer runs **targeted searches** (Tavily) based on what the LLM is seeing:

- "ICE enforcement Twin Cities 2026"
- "Cedar-Riverside immigration arrests"
- "Minnesota rent assistance demand spike"

Same tool the system already uses for source discovery, different purpose. Search results get snapshotted like any other evidence page.

#### Path 3: Social Media as Primary Evidence

Social media posts are where the raw, unfiltered statements live. A church's Instagram story saying "we're seeing so many families who can't go to work right now" is a direct statement from the source — arguably more honest than a polished news article.

The system already scrapes social media via Apify (Tier 2 data). Currently Tier 2 is "never served to consumers." But as evidence for an Activity, it's not being served directly — it's being **cited**. "Grace Church posted on Instagram: 'families afraid to leave their homes.'" The citation points to the post. The Activity aggregates the pattern.

Social media posts are particularly powerful evidence because they're:
- **Timestamped** — you know when the statement was made
- **Attributable** — a real org or person said it
- **Unedited** — raw language, not PR-filtered
- **Volume-based** — 10 different orgs saying the same thing is strong corroboration

And since the system is already crawling these accounts, the investigation layer doesn't need extra scraping. It queries what's already been captured in the database.

The evidence hierarchy for an Activity:

1. **The org's own statement** — their post, their words, explicitly stating why (strongest)
2. **Community social media posts** — other orgs/people in the area saying the same thing
3. **Published news reporting** — journalistic coverage connecting the dots
4. **Government records** — institutional data already in the system (USAspending, EPA, etc.)

#### Path 0: Check the Database First

Before any crawling or searching, check if an existing Activity already matches via embedding similarity. If there's already an "ICE enforcement in Twin Cities" Activity in the system, just link the new signal to it — no investigation needed. This is the primary cost control mechanism.

### The Critical Constraint: Explicit Links Only

**Every link from a signal to an Activity must be grounded in explicit statements from the evidence.** The LLM does not infer causation — it finds where causation is already stated.

```
VALID:
Church post says "helping families afraid to leave home due to immigration enforcement"
  → The church explicitly stated the cause. Link is grounded.

News article says "Twin Cities churches stepping up as ICE operations increase"
  → Published reporting explicitly connects churches to ICE. Link is grounded.

Community Instagram post says "another ICE van spotted on Lake Street today"
  → Direct eyewitness statement, timestamped, attributable. Evidence is grounded.

INVALID:
Church says "rent relief available" (no stated reason)
  + LLM finds news about ICE in the same city
  → The church never said it's because of ICE. This is an assumption.
```

The invalid case might *actually* be about ICE. But a single signal can't claim that without explicit evidence. The church that just says "rent relief available" stays as a `give` signal with no Activity link — because nobody stated why.

However, **cluster detection** (see below) can catch what individual signals miss. If 10 churches in the same neighborhood are all posting rent/food/grocery signals in the same week, the cluster is the anomaly — even if no single signal mentions ICE. The investigation triggers at the cluster level and searches for the explicit evidence that explains the pattern.

This constraint keeps the system honest. Activities are **aggregations of explicit, cited statements** — not inferences the system makes. The LLM's job is to find where causation is already stated and aggregate those statements, not to speculate.

### Two Investigation Triggers

#### Trigger 1: Content Hints at Deeper (Single Signal)

The existing mechanism — the LLM detects crisis language, causal framing, or unusual context during extraction. Works for signals that explicitly carry weight beyond the surface broadcast.

#### Trigger 2: Cluster Detection (Batch Pattern Scan)

A periodic batch job scans for **geographic + temporal + type anomalies** — unusual concentrations of signals that individually look normal but collectively indicate something is happening:

- "12 new `ask` and `give` signals in Cedar-Riverside this week, vs. a baseline of 2/week"
- "4 different orgs in the same neighborhood all posting about legal aid within 3 days"
- "Sudden spike in `event` signals related to 'rights' or 'know your rights' in one area"

No single signal triggered investigation. The cluster did. The investigation then searches for *why* this area is lighting up — and finds the news articles, social media posts, government records that explain it. The evidence is still explicit, but the trigger is the pattern.

This solves the biggest gap in single-signal detection: normal-looking signals that only become meaningful in aggregate.

### Agentic Investigation

The investigation isn't a fixed pipeline — it's an **agent loop**. The LLM gets the triggering signal (or cluster), existing context from the DB, and a set of tools:

- `follow_link(url)` — crawl and snapshot a page
- `search(query)` — Tavily search, snapshot results
- `query_signals(area, time, type)` — check existing signals in the DB
- `query_social(entity, area, time)` — check already-captured social media
- `query_entities(name)` — check institutional records (USAspending, EPA, etc.)
- `recommend_source(url, reason)` — suggest a new source to monitor

The LLM decides what to do next based on what it's found so far:

```
"I found a news article mentioning Acme Corporation polluting the river."
  → query_entities("Acme Corporation")
  → Found EPA ECHO violation already in the system.
  → query_signals(area=riverside, time=last_30d, type=event)
  → Found 3 river cleanup events nearby.
  → follow_link(cleanup_org_website)
  → Their site says "water quality deteriorating due to upstream industrial discharge."
  → Enough evidence. Synthesize Activity.
```

The agent follows the evidence wherever it leads — within bounded tool calls (max ~10 tool calls per investigation) and bounded depth (max 3 link hops). This is more powerful than a fixed sequence because the LLM adapts its investigation based on what it discovers.

### Adversarial Validation

Before an Activity is created, a second LLM pass pressure-tests it through a **structured validation checklist**:

1. **Quote check**: For every signal-to-Activity link, copy the exact quote from the evidence that states the connection. If you can't find an explicit quote, the link fails.
2. **Counter-hypothesis**: Propose at least one alternative explanation for the signal pattern. If a simpler explanation exists, note it.
3. **Evidence sufficiency**: Are there at least 3 independent sources? At least 2 different evidence types (e.g., org statement + news, not just 3 org statements)?
4. **Scope check**: Is the Activity conclusion proportional to the evidence? "ICE conducting operations" is proportional. "Federal government targeting immigrants" is broader than the evidence supports.

The investigator LLM is incentivized to find a story. The validator LLM is incentivized to break it. The structured checklist prevents rubber-stamping — the validator must do specific work, not just assess vibes.

If the validator rejects: no Activity created. If the validator identifies a valid counter-hypothesis: both hypotheses can be noted, with the evidence for each. The user sees the strongest explanation and the alternative.

### Output: A Damn Good Answer

An Activity is not a chain of reasoning steps. It's a **conclusion with cited evidence**. The bar: enough depth and explicitly-stated evidence to satisfy a real answer for why signals exist.

- **Conclusion**: A factual statement about what's happening ("ICE conducting street-level enforcement operations in Twin Cities")
- **Evidence**: Specific content the LLM read, with citations — org posts, social media statements, news articles, government records. Every piece explicitly states or directly documents the claim.
- **Linked signals**: The surface broadcasts this Activity explains, each with an explicit causal link from the evidence

The system doesn't say "ICE is causing harm." It says: "Sahan Journal reported ICE arrests in Cedar-Riverside. Grace Church posted 'families are afraid to leave their homes.' ACLU MN posted know-your-rights resources for immigration enforcement. USAspending shows a $50M ICE contract awarded to GEO Group." The Activity aggregates these statements. The reader draws the conclusion.

### Graph Convergence: Many Signals, One Activity

The most powerful output is when multiple signals from different entities converge on the same Activity:

```
Signals (surface)              Activity (underlying)
─────────────────              ─────────────────────
rent relief (give)         ──→ ICE street operations in Twin Cities
grocery delivery (give)    ──→ ICE street operations in Twin Cities
know-your-rights (event)   ──→ ICE street operations in Twin Cities
ride sharing (give)        ──→ ICE street operations in Twin Cities

river cleanup (event)      ──→ Industrial discharge from Acme Corp
EPA violation (informative)──→ Industrial discharge from Acme Corp
water testing (event)      ──→ Industrial discharge from Acme Corp
```

Each Activity becomes a node in the graph. Signals point to Activities. Activities can also point to other Activities (ICE operations → federal immigration policy shift). The graph reveals the structure of what's happening in a community at a level no individual signal can.

### Bidirectional Flow: The Chain Works Backwards

Activities aren't just explained by signals — they **generate** signals. When the system discovers ICE enforcement activity and adds new sources to monitor, those sources produce new signals: "Cedar-Riverside families need legal aid," "church needs volunteers for emergency fund." These signals link back to the same Activity.

The Activity becomes a **hub** with two sides:

```
Evidence signals ──→ Activity ──→ Response signals

Sahan Journal: ICE arrests       ICE enforcement     Grace Church: needs volunteers (ask)
USAspending: ICE contract    ──→ in Twin Cities  ──→ St. Mary's: needs food donations (ask)
Community post: ICE van            (the why)         ACLU MN: needs pro bono lawyers (ask)
spotted on Lake St                                   Mutual aid: needs ride volunteers (ask)
```

**Left side:** why it's happening — evidence signals (informative, news, government records).
**Right side:** who's responding and what they need — response signals (asks, gives, events from orgs on the ground).

A user who cares about this Activity sees both:
- "Here's what's happening" (the evidence)
- "Here's who's helping and what they need" (the asks)

This is where the system becomes deeply actionable. Individual asks — "church needs volunteers" — are useful on their own. But connected to an Activity, they become part of a larger picture. A user doesn't just see a volunteer ask — they see *why* the church needs volunteers, *who else* is responding, and *what else* is needed. The Activity is the context that transforms scattered asks into a coordinated picture of community response.

The self-expanding awareness loop completes the cycle:

```
Signal hints at something deeper
  → Investigation produces Activity + new sources
    → New sources produce new signals
      → New signals link back to the Activity (evidence + response)
        → Activity page shows: what's happening + who needs help
          → User finds where to show up
```

The mycorrhizal network in action — the system detects a threat, grows toward it, and surfaces where help is needed.

## What an Activity Is (and Isn't)

**An Activity is:**
- An aggregation of explicit, cited statements about what's happening
- Grounded in evidence someone actually said or published — org posts, social media, news, government records
- Cited with specific sources and URLs
- Neutral — "ICE conducting operations" not "ICE terrorizing community"
- A convergence point for multiple signals
- Validated by an adversarial LLM pass before creation

**An Activity is not:**
- An inference (every causal link is explicitly stated in the evidence)
- An entity (it's a condition/event, not an organization)
- Editorial judgment (the system aggregates what sources say, it doesn't draw conclusions)
- Speculation (if nobody explicitly stated the cause, no Activity is created)
- A score or ranking (no severity, no urgency — just facts)

**The defensibility test:** Can the system say "we're just indexing what these sources explicitly state"? If yes, the Activity is valid. If the system is connecting dots that nobody else connected, it's editorializing — and the Activity should not exist.

## Relationship to Existing Architecture

This layer sits on top of the signal pipeline:

```
Source → Scrape → page_snapshot → Signal Extraction → signals table
                                        │
                              ┌─────────┴──────────┐
                              │                    │
                     Single-signal trigger    Cluster trigger
                     (content hints deeper)  (batch pattern scan)
                              │                    │
                              └─────────┬──────────┘
                                        │
                                        ▼
                                  Why Investigation (Agent Loop)
                                        │
                                        ├─ Check DB for existing Activity match (embeddings)
                                        ├─ If match: link signal → existing Activity, done
                                        ├─ If no match: agentic investigation
                                        │   ├─ LLM chooses tools based on findings:
                                        │   │   follow_link(), search(), query_signals(),
                                        │   │   query_social(), query_entities()
                                        │   ├─ Max ~10 tool calls, max 3 link hops
                                        │   ├─ Snapshot all evidence pages
                                        │   └─ Require explicit causal statements
                                        ├─ Adversarial validation (structured checklist)
                                        ├─ If validated: create Activity + link evidence
                                        ├─ recommend_source() for new sources found
                                        │
                                        ▼
                                  activities table
                                        │
                                        ├─ signal_activities (many-to-many)
                                        ├─ activity_evidence (cited snapshots + URLs)
                                        └─ activity_relationships (activity → activity, max 2 levels)
```

Every page the LLM follows gets snapshotted — the evidence chain is fully auditable.

Activities don't replace signals. They're a layer above — explaining why signals exist. The consumer app can show both: "Here's what's happening (signals). Here's why (activities)."

## Activity Lifecycle

Activities describe conditions that change. They need a temporal dimension.

### Status

- **emerging** — newly created, evidence is fresh but thin (minimum threshold met). The system is still gathering signals.
- **active** — multiple signals linked, evidence is strong, new signals still arriving. This is happening now.
- **declining** — signal velocity is dropping. No new signals linked in 14+ days. Evidence is aging.
- **resolved** — no new signals in 30+ days AND newest evidence is 30+ days old. The condition appears to have passed.

### Signal Velocity

Track how fast new signals link to an Activity. A high-velocity Activity (5 new signals this week) is actively unfolding. A zero-velocity Activity for 30 days is likely resolved. This is mechanical — no LLM judgment needed.

### Re-Investigation

When an Activity is `active` and new signals keep linking to it, periodically re-investigate (e.g., weekly) to:
- Update the evidence with newer sources
- Check if the situation has changed (escalated, de-escalated, shifted)
- Refresh the Activity's conclusion if the evidence warrants it
- Find new sources that have emerged since the original investigation

### Expiry

Activities don't delete — they transition to `resolved`. Historical Activities are still valuable: "ICE conducted enforcement operations in Cedar-Riverside, Feb-April 2026." The evidence remains. The status just reflects that it's no longer actively producing signals.

## Activity Graph Depth

Activities can link to other Activities when the evidence explicitly supports it:

```
Activity: ICE enforcement in Twin Cities
  → linked to: Federal immigration policy executive order (Jan 2026)
    (evidence: news article explicitly connecting local operations to federal policy)

Activity: Industrial discharge from Acme Corp
  → linked to: EPA enforcement budget cuts
    (evidence: news article explicitly connecting reduced oversight to violations)
```

**Rules for Activity → Activity links:**
- Same explicit-evidence constraint. A news article must say "local ICE operations follow the January executive order." The system can't infer the policy connection.
- **Max 2 levels deep.** Activity → parent Activity → grandparent Activity. Beyond that, you're in geopolitics, which is real but not actionable at the community level.
- Each level requires its own adversarial validation.

This gives the graph meaningful depth without becoming a conspiracy board. Two levels is enough to connect local conditions to regional/national causes when the evidence explicitly supports it.

## Pressure Testing

| Scenario | Surface Signals | Activity |
|----------|----------------|----------|
| ICE enforcement | Rent relief, legal clinics, grocery delivery, ride sharing | ICE street-level operations in Twin Cities |
| Factory pollution | River cleanup events, water testing volunteers, EPA violations | Industrial discharge from Acme Corp |
| Housing crisis | Shelter overflow, mutual aid for rent, housing rights workshops | Rental market displacement in North Minneapolis |
| School closure | Childcare asks, after-school program gives, parent organizing events | District closing Roosevelt Elementary |
| Opioid surge | Narcan training events, harm reduction gives, memorial fundraisers | Fentanyl supply increase in Hennepin County |

In each case: the signals individually are useful. The Activity reveals the story.

## Self-Expanding Awareness

The investigation layer doesn't just produce Activities — it produces **recommendations for the system to expand its own awareness.** When the LLM follows a trail and finds something real, it should be able to say: "the system should be watching this."

### How It Works

During investigation, the LLM encounters sources and topics that aren't currently in the system. Instead of just citing them as evidence and moving on, it can recommend:

1. **New sources to monitor** — "Sahan Journal covers the Cedar-Riverside community and published reporting on ICE enforcement. The system should add this as a source."
2. **New Tavily queries to run on a cadence** — "Set up a recurring search for 'ICE enforcement Twin Cities' to catch future developments."
3. **New social media accounts to follow** — "This mutual aid group's Instagram is posting real-time updates about community conditions. Add it to the scrape list."
4. **New entities to track** — "This news article mentions CoreCivic as an ICE contractor. The system should create this entity and check USAspending for their contracts."

### The Feedback Loop

This creates a virtuous cycle:

```
Signal detected
  → Investigation finds evidence + new sources
    → New sources added to the system
      → New sources produce new signals
        → New signals trigger new investigations
          → More evidence, more sources
```

The system gets smarter about a topic the more it investigates. The first church posting about rent relief triggers an investigation that discovers Sahan Journal, which gets added as a source, which produces more signals about the community, which link to the same Activity, which strengthens the evidence base.

### Guardrails

The LLM recommends. The system (or an admin) decides:

- **Auto-add with review**: New sources are created with `is_active = true` but flagged for admin review. They start producing signals immediately — if they're low value, adaptive cadence backs them off naturally.
- **Query caps**: Recurring Tavily queries are capped (e.g., max 5 active investigation queries per Activity). They expire when the Activity goes stale.
- **Relevance bound**: The LLM can only recommend sources directly encountered during investigation — not speculative sources it thinks might exist. It found Sahan Journal because a church linked to it. It doesn't hallucinate "there's probably a community newspaper in Cedar-Riverside."

This is the mycorrhizal metaphor fully realized. The network doesn't just transmit signals — it grows toward where the signals are.



| Risk | Resolution |
|------|-----------|
| **Confabulation** — LLM constructs plausible but false causal narratives | Adversarial validation: second LLM pass tries to break the hypothesis before Activity is created |
| **Editorial bias** — Activities are inferences, which is editorializing | Explicit links only: every causal link must be grounded in stated evidence. The system aggregates what sources say, not what it infers. Same defense as signals — "we're indexing what was published." |
| **False convergence** — unrelated signals grouped under one Activity | Each signal-to-Activity link pressure-tested individually. If the evidence doesn't explicitly connect THIS signal to THIS activity, no link. |
| **Everything is connected** — conspiracy-board risk | The causal link must be EXPLICIT in the content. No inferred connections. If nobody stated the connection, the system can't claim it. |
| **Cost explosion** — investigation is expensive | Check DB embeddings first (free). Query already-captured social media (free). Only crawl/search when needed. Cap at 3 crawls + 1 search per investigation. |
| **Weaponization** — Activities used as editorial claims | Point to sources of truth: news articles, social media posts, government records. The Activity is a collection of citations, not a claim. |

## Open Questions

- **Minimum evidence threshold**: The adversarial validator requires 3+ independent sources and 2+ evidence types. Is this the right bar? Too high and you miss emerging Activities. Too low and you get noise.
- **Consumer UX**: How do Activities appear in the consumer app? Options: a "what's happening here" map layer, contextual cards alongside related signals, a dedicated Activity feed, or entity pages showing linked Activities. Probably all of the above — but what's the MVP?
- **Cluster detection tuning**: What constitutes an anomalous cluster? "12 asks in Cedar-Riverside vs. baseline of 2/week" — how do we establish baselines per area? Rolling average? Per-neighborhood thresholds?
- **Deduplication edge cases**: Embedding similarity catches most duplicates at the DB-check step. But what about related-but-distinct Activities? "ICE enforcement in Cedar-Riverside" vs. "ICE enforcement in Twin Cities" — merge or keep separate? Probably geographic scoping, but needs design.
- **Investigation queue priority**: When multiple signals/clusters are flagged for investigation simultaneously, how do we prioritize? Signal velocity (fast-moving clusters first)? Geographic density? Recency?

## Next Steps

-> `/workflows:plan` for implementation details — data model, investigation prompt, graph storage, admin UI for reviewing Activities
