---
date: 2026-03-13
topic: actionable-situation-briefings
---

# Actionable Situation Briefings

## What We're Building

Transform the `/situations/:id` page from a bare stats dashboard into a neighbor-written briefing that explains what's happening, how people are responding, and what's needed — then gives readers direct calls to action to help.

The narrative is generated at weave time (not on every page load). Re-weaving updates it when new signals arrive. The situation page becomes the "diving board" that briefs people and launches them into action.

## The Problem

Today a situation shows: headline, lede (2-3 sentences), temperature gauges, counts, and a dispatch table. None of the substance — the actual gatherings, help requests, resources, concerns — is visible. The page informs but doesn't activate. The member signals that make a situation actionable are locked in Neo4j with no GraphQL path to surface them.

## Chosen Approach: Briefing Layout (Approach A)

The page reads top-to-bottom like an article a neighbor wrote:

### 1. Narrative Section (generated at weave time)
- **What's happening** — the situation explained in plain, warm language
- **How people are responding** — who's organizing, what's already underway
- **What's needed** — explicit asks: "help with rent relief", "sign up to drive", "be a watcher"
- Stored as a new field on the Situation node (e.g. `briefing_body` or expanded `structured_state`)
- Tone: like a neighbor wrote it, not a press release

### 2. What Can You Do (data-driven CTAs from member signals)
- Direct action cards extracted from signal data:
  - HelpRequest → "Help with [whatNeeded]" + link (actionUrl)
  - Gathering → "Join [title] on [date]" + link (actionUrl)
  - Resource → "[title] is available" + link (actionUrl), availability/eligibility
- These come from querying the member signals, not from the LLM narrative
- Each card is a diving board into the specific signal

### 3. Context Sections (member signals grouped by type)
- **Concerns** — what people are worried about, with severity
- **Conditions** — observed conditions, measurements, affected scope
- **Announcements** — official info, source authority, effective dates
- Each links to the signal detail page for deeper dive

### 4. Dispatches Timeline (existing, kept as-is)
- Update history showing how the situation evolved

### 5. Metadata Footer
- Temperature components (moved from hero position to supporting)
- Arc, clarity, dates, location

## Key Decisions

- **Narrative generated at weave time, not on page load**: Weaving IS the narrative generation. Cost-effective, consistent. Re-weave button refreshes it.
- **Expand ClusterNarrative**: Currently produces `{ headline, lede, structured_state }`. Needs to produce a full briefing body with the four sections above, plus extracted CTAs.
- **New GraphQL resolver needed**: `situation(id) { signals { ... } }` — traverse `(Situation)<-[:PART_OF]-(Signal)` in Neo4j. Group by signal type on the frontend.
- **Tone directive in LLM prompt**: "Write as if a caring neighbor is briefing their community. Be warm, direct, and action-oriented. Don't use bureaucratic language."
- **CTAs are data-driven, not LLM-generated**: The "What can you do" section pulls action_url, whatNeeded, organizer, availability directly from signals. The LLM writes the narrative; the data drives the actions.
- **Admin-first, public-ready shape**: This design works for both the admin coordinator view and a future public-facing version.

## Data Requirements

### Already exists:
- SituationNode with headline, lede, structured_state
- Member signals connected via `(Signal)-[:PART_OF]->(Situation)`
- Signal types with actionable fields (action_url, what_needed, stated_goal, organizer, availability, etc.)
- Dispatch timeline
- Re-weave capability on cluster page

### Needs to be built:
- **GraphQL resolver**: signals for a situation (reader method + resolver)
- **Enhanced ClusterNarrative schema**: add `briefing_body` (or sections) to what the LLM produces at weave time
- **Store briefing on Situation node**: new Neo4j property or expand structured_state
- **Updated weave prompt**: produce the full neighbor-tone briefing, not just headline/lede
- **Frontend**: SituationDetailPage rewrite with briefing layout, CTA cards, grouped signals

## Open Questions

- Should the briefing body be markdown or structured JSON sections? (Markdown is simpler to render, JSON gives more control over layout)
- Do we expose `structured_state` fields like `root_cause_thesis` and `key_actors` in the briefing, or keep them internal?
- Should CTAs have a priority/ordering? (e.g., urgent help requests first)
- How do we handle situations with few signals? (e.g., a situation woven from 2 signals — the briefing might feel thin)

## Next Steps

→ `/workflows:plan` for implementation details
