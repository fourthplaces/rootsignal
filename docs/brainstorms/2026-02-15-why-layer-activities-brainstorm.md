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

### Investigation: Crawl the Org's Own Content

The system already has links to the entity's website, social media accounts, and related pages. The investigation layer:

1. Crawls the entity's linked pages — website, Instagram, Facebook, X
2. Follows relevant links from those pages (news articles, partner orgs)
3. Checks existing signals in the graph — what else is happening in this area/time?
4. Feeds all this context to the LLM

The LLM isn't speculating. It's reading what the org and its community are actually saying, the same way a human researcher would follow the trail.

### Output: A Damn Good Answer

An Activity is not a chain of reasoning steps. It's a **conclusion with evidence**. The bar: enough depth and evidence to satisfy a real answer for why this signal exists.

- **Conclusion**: A factual statement about what's happening ("ICE conducting street-level enforcement operations in Twin Cities")
- **Evidence**: Specific content the LLM read, with citations (URLs, source names)
- **Linked signals**: The surface broadcasts this Activity explains

The depth isn't fixed at N levels. The LLM goes deep enough to produce something substantive and cited. Sometimes that's one hop ("factory upstream dumped chemicals" explains a river cleanup). Sometimes it's several ("families hiding → ICE enforcement → federal contract → detention company").

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

## What an Activity Is (and Isn't)

**An Activity is:**
- A factual statement about what's happening in the world
- Derived from evidence the LLM actually read
- Cited with specific sources
- Neutral — "ICE conducting operations" not "ICE terrorizing community"
- A convergence point for multiple signals

**An Activity is not:**
- A signal (nobody broadcast it — it's inferred from evidence)
- An entity (it's a condition/event, not an organization)
- Editorial judgment (the system doesn't say this is good or bad)
- Speculation (every claim backed by cited content)
- A score or ranking (no severity, no urgency — just facts)

## Relationship to Existing Architecture

This layer sits on top of the signal pipeline:

```
Source → Scrape → page_snapshot → Signal Extraction → signals table
                                        │
                                        ├─ LLM detects "hints at something deeper"
                                        │
                                        ▼
                                  Why Investigation
                                        │
                                        ├─ Crawl entity's linked pages
                                        ├─ Check existing signals nearby
                                        ├─ LLM synthesizes evidence
                                        │
                                        ▼
                                  activities table
                                        │
                                        ├─ signal_activities (many-to-many)
                                        ├─ activity_evidence (citations)
                                        └─ activity_relationships (activity → activity)
```

Activities don't replace signals. They're a layer above — explaining why signals exist. The consumer app can show both: "Here's what's happening (signals). Here's why (activities)."

## Pressure Testing

| Scenario | Surface Signals | Activity |
|----------|----------------|----------|
| ICE enforcement | Rent relief, legal clinics, grocery delivery, ride sharing | ICE street-level operations in Twin Cities |
| Factory pollution | River cleanup events, water testing volunteers, EPA violations | Industrial discharge from Acme Corp |
| Housing crisis | Shelter overflow, mutual aid for rent, housing rights workshops | Rental market displacement in North Minneapolis |
| School closure | Childcare asks, after-school program gives, parent organizing events | District closing Roosevelt Elementary |
| Opioid surge | Narcan training events, harm reduction gives, memorial fundraisers | Fentanyl supply increase in Hennepin County |

In each case: the signals individually are useful. The Activity reveals the story.

## Open Questions

- **Deduplication**: When two investigation chains arrive at the same root cause, how do we merge them into one Activity? Embedding similarity on the conclusion text? Manual admin merge?
- **Staleness**: Activities describe conditions. Conditions change. How do we detect when an Activity is no longer active? Re-investigation on a cadence? Signal volume decay?
- **Confidence**: Some Activities will be well-evidenced (5+ sources). Some will be thin (one org's Facebook post). Should there be a minimum evidence threshold before an Activity is surfaced?
- **Cost**: Each "why" investigation means additional crawling + LLM calls. What's the expected volume of flagged signals vs. total signals? 5%? 20%?
- **Consumer UX**: How do Activities appear in the consumer app? A separate view? Contextual — shown alongside related signals? A "what's happening here" map layer?
- **Activity-to-Activity links**: ICE operations → federal immigration policy. Factory pollution → deregulation. How deep does the graph go? Is there a practical limit?

## Next Steps

-> `/workflows:plan` for implementation details — data model, investigation prompt, graph storage, admin UI for reviewing Activities
