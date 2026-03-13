---
title: "feat: Actionable Situation Briefings"
type: feat
date: 2026-03-13
---

# Actionable Situation Briefings

## Overview

Transform `/situations/:id` from a bare stats page into a neighbor-written briefing that explains what's happening, how people are responding, and what's needed — then gives readers direct calls to action. The narrative is generated at weave time and stored on the Situation node. Member signals are surfaced via a new GraphQL resolver so the frontend can build type-grouped CTA cards.

## Problem Statement

Today the situation page shows: headline, lede (2-3 sentences), temperature bars, counts, and a dispatch table. The substance — the actual gatherings you can attend, help requests you can answer, resources available — is invisible. The page informs but doesn't activate. There's no GraphQL path from a situation to its member signals, and the weave only produces a minimal headline/lede.

## Proposed Solution

Three layers, buildable independently:

1. **Data access** — new `signals` resolver on `GqlSituation` returning typed member signals via `PART_OF` edges
2. **Richer narrative** — expand `ClusterNarrative` to produce a `briefing_body` at weave time, stored on the Situation node
3. **Frontend rewrite** — restructure the page as a briefing with narrative, CTA cards, context sections, and dispatches

## Technical Approach

### Phase 1: Signals-for-Situation (Data Access)

Surface member signals through GraphQL so the frontend can build CTA cards.

**Files to change:**

#### `rootsignal-graph/src/reader.rs` — new reader method

Add `signals_for_situation` to `PublicGraphReader`. Follow the `dispatches_for_situation` pattern (lines 2511-2538).

```rust
// rootsignal-graph/src/reader.rs
pub async fn signals_for_situation(
    &self,
    situation_id: &Uuid,
    limit: u32,
) -> Result<Vec<TypedSignalNode>> {
    // MATCH (sig)-[:PART_OF]->(s:Situation {id: $id})
    // WHERE sig:Gathering OR sig:Resource OR sig:HelpRequest
    //    OR sig:Announcement OR sig:Concern OR sig:Condition
    // RETURN sig, labels(sig) AS labels
    // ORDER BY sig.extracted_at DESC
    // LIMIT $limit
}
```

Returns full typed signal nodes (not `SignalBrief`) because CTAs need type-specific fields: `action_url`, `what_needed`, `organizer`, `starts_at`, `availability`, `urgency`, `severity`, `subject`.

The reader already has `row_to_gathering`, `row_to_resource`, etc. — use `labels` to dispatch to the right parser. Follow the pattern in `all_signals` or `signals_near` which return the full union.

- [x] Add `signals_for_situation` method to `PublicGraphReader` in `reader.rs`
- [x] Return typed signal nodes using existing `row_to_*` parsers

#### `rootsignal-api/src/graphql/types.rs` — new resolver on GqlSituation

Follow the `dispatches()` resolver pattern exactly (lines 1320-1332):

```rust
// rootsignal-api/src/graphql/types.rs — on impl GqlSituation
async fn signals(
    &self,
    ctx: &Context<'_>,
    #[graphql(default = 50)] limit: u32,
) -> Result<Vec<GqlSignal>> {
    let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
    let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
    let signals = reader
        .signals_for_situation(&self.0.id, limit.min(100))
        .await?;
    Ok(signals.into_iter().map(into_gql_signal).collect())
}
```

`GqlSignal` is the existing union type (lines 312-333) covering all 6 signal types. The frontend already handles this via the `SIGNAL_FIELDS` fragment in `queries.ts`.

- [x] Add `signals()` resolver to `GqlSituation` following `dispatches()` pattern
- [x] Map typed signal nodes to `GqlSignal` union variants

#### `admin-app/src/graphql/queries.ts` — extend SITUATION_DETAIL query

Add `signals` field using the existing `SIGNAL_FIELDS` fragment:

```graphql
# In SITUATION_DETAIL query
signals(limit: 50) {
  ${SIGNAL_FIELDS}
}
```

- [x] Add `signals` field to `SITUATION_DETAIL` query

### Phase 2: Briefing Narrative (Enhanced Weave)

Expand what the LLM produces at weave time from headline/lede to a full briefing.

**Files to change:**

#### `rootsignal-scout/src/domains/cluster_weaving/activities.rs` — expand ClusterNarrative

```rust
// rootsignal-scout/src/domains/cluster_weaving/activities.rs
#[derive(Deserialize, JsonSchema)]
struct ClusterNarrative {
    headline: String,
    lede: String,
    briefing_body: String,  // NEW — full narrative in markdown
    structured_state: serde_json::Value,
}
```

Update `build_first_weave_prompt` system prompt:

```
You are a caring neighbor briefing your community about a developing situation.
Given a cluster of local signals, write a briefing that:

1. headline: One sentence capturing the situation
2. lede: 2-3 sentences of context
3. briefing_body: A 3-5 paragraph narrative in markdown covering:
   - What's happening (the core situation)
   - How people are responding (who's organizing, what's underway)
   - What's needed (explicit asks — "volunteers needed for X", "donations needed for Y")
   Write as if explaining to a neighbor. Be warm, direct, action-oriented.
   Use **bold** for key details. No bureaucratic language.
4. structured_state: { root_cause_thesis, key_actors, status }
```

- [x] Add `briefing_body: String` field to `ClusterNarrative` struct
- [x] Update `build_first_weave_prompt` with neighbor-tone instructions
- [ ] Update `build_delta_prompt` to also refresh the briefing body on re-weave

#### `rootsignal-common/src/system_events.rs` — add field to SituationIdentified

```rust
// In SystemEvent::SituationIdentified variant
briefing_body: Option<String>,  // NEW
```

Use `Option<String>` (not bare `String`) so situations created before this change don't break, and so the system degrades gracefully if LLM extraction fails.

- [x] Add `briefing_body: Option<String>` to `SituationIdentified` event

#### `rootsignal-common/src/types.rs` — add field to SituationNode

```rust
// In SituationNode
pub briefing_body: Option<String>,
```

- [x] Add `briefing_body: Option<String>` to `SituationNode`

#### `rootsignal-graph/src/projector.rs` — store briefing on Situation node

In the `SituationIdentified` projection (lines 1659-1738), add `briefing_body` to the MERGE SET clause:

```cypher
SET s.briefing_body = $briefing_body
```

- [x] Add `briefing_body` property to SituationIdentified projection

#### `rootsignal-graph/src/reader.rs` — read briefing from Neo4j

In `row_to_situation` (lines 2643-2713), extract the new field:

```rust
briefing_body: node.get::<String>("briefing_body").ok(),
```

- [x] Add `briefing_body` extraction to `row_to_situation`

#### `rootsignal-api/src/graphql/types.rs` — expose via GraphQL

```rust
// On impl GqlSituation
async fn briefing_body(&self) -> Option<&str> {
    self.0.briefing_body.as_deref()
}
```

- [x] Add `briefing_body` resolver to `GqlSituation`

### Phase 3: Frontend Rewrite

Restructure `SituationDetailPage.tsx` as a briefing layout.

#### `admin-app/src/pages/SituationDetailPage.tsx`

New page structure (top to bottom):

```
┌─────────────────────────────────────────────┐
│ Breadcrumb: Situations / Headline           │
├─────────────────────────────────────────────┤
│ HEADER                                      │
│ Headline (h1) + Arc badge + Location        │
│ Lede (subtitle text)                        │
├─────────────────────────────────────────────┤
│ BRIEFING NARRATIVE                          │
│ briefing_body rendered as markdown           │
│ (or fallback to lede if no briefing_body)   │
├─────────────────────────────────────────────┤
│ WHAT CAN YOU DO                             │
│ ┌──────────┐ ┌──────────┐ ┌──────────┐     │
│ │ Join     │ │ Help w/  │ │ Available│     │
│ │ cleanup  │ │ rent     │ │ at food  │     │
│ │ Mar 15   │ │ relief   │ │ bank     │     │
│ │ [Link →] │ │ [Link →] │ │ [Link →] │     │
│ └──────────┘ └──────────┘ └──────────┘     │
│ Gatherings     HelpRequests    Resources    │
├─────────────────────────────────────────────┤
│ CONTEXT                                     │
│ Concerns (severity badges, subject, opposing)│
│ Conditions (measurement, affected scope)    │
│ Announcements (source authority, eff. date) │
├─────────────────────────────────────────────┤
│ DISPATCHES (existing table, kept as-is)     │
├─────────────────────────────────────────────┤
│ METADATA FOOTER                             │
│ Temperature components, clarity, dates      │
└─────────────────────────────────────────────┘
```

**CTA Card logic by signal type:**

| Signal Type | Card Title | Card Detail | Action |
|---|---|---|---|
| Gathering | "Join: {title}" | date, organizer, location | action_url → "Go" |
| HelpRequest | "Help: {whatNeeded \|\| title}" | urgency badge, stated_goal | action_url → "Respond" |
| Resource | "{title}" | availability, eligibility | action_url → "Details" |

**Context card logic:**

| Signal Type | Display |
|---|---|
| Concern | severity badge + subject + opposing |
| Condition | severity badge + measurement + affected_scope |
| Announcement | source_authority + effective_date + subject |

**Edge cases:**
- No `briefing_body` → show lede only, no narrative section
- No signals of a given type → skip that CTA/context section
- Signal with no `action_url` → show card without link button
- Many signals (20+) → show first 6 CTAs with "Show all N signals" expand

**Dependencies:**
- `react-markdown` for rendering briefing_body (or simple `dangerouslySetInnerHTML` if markdown is pre-rendered)

- [x] Install markdown rendering dependency (or use simple prose styling)
- [x] Rewrite `SituationDetailPage.tsx` with briefing layout
- [x] Build CTA card component for actionable signals
- [x] Build context card component for informational signals
- [x] Group signals by type and render appropriate cards
- [x] Graceful fallbacks for missing data (no briefing, no CTAs, empty sections)
- [x] Update `SITUATION_DETAIL` query to include `signals` and `briefingBody`

## Acceptance Criteria

- [x] `situation(id) { signals { ... } }` returns typed member signals via GraphQL
- [x] Weaving a cluster produces a `briefing_body` on the resulting Situation
- [ ] Re-weaving updates the briefing_body
- [x] Situation page shows narrative section with briefing_body
- [x] Situation page shows CTA cards grouped by type (Gatherings, HelpRequests, Resources)
- [x] CTA cards link to action_url when available
- [x] Context sections show Concerns/Conditions/Announcements with relevant details
- [x] Page degrades gracefully when sections have no data
- [x] Temperature/metadata moved to footer position
- [x] Admin app compiles with no type errors

## Dependencies & Risks

**Dependencies:**
- Phase 1 (data access) is independent — can ship alone and immediately improves the page
- Phase 2 (narrative) requires re-weaving existing situations to populate briefing_body
- Phase 3 (frontend) depends on Phase 1. Can partially ship without Phase 2 (just skip narrative section)

**Risks:**
- LLM prompt quality: the "neighbor tone" narrative may need iteration. Start simple, improve the prompt based on real output.
- `briefing_body` on existing situations will be `None` until re-woven. Frontend must handle this gracefully.
- Signal count per situation varies. A situation with 2 signals will have a thin CTA section. That's OK — the briefing narrative fills the gap.

## References

- **Brainstorm:** `docs/brainstorms/2026-03-13-actionable-situation-briefings-brainstorm.md`
- **Cluster weave plan:** `docs/plans/2026-03-12-feat-cluster-detail-weave-workflow-plan.md`
- **Weave activities:** `rootsignal-scout/src/domains/cluster_weaving/activities.rs`
- **SituationIdentified event:** `rootsignal-common/src/system_events.rs:270`
- **SituationNode type:** `rootsignal-common/src/types.rs:1497`
- **GqlSituation + dispatches resolver:** `rootsignal-api/src/graphql/types.rs:1245-1332`
- **Situation projector:** `rootsignal-graph/src/projector.rs:1659-1738`
- **Reader situation_by_id:** `rootsignal-graph/src/reader.rs:2322`
- **Frontend page:** `admin-app/src/pages/SituationDetailPage.tsx`
- **GraphQL query:** `admin-app/src/graphql/queries.ts:1143`
- **Institutional learning:** `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md` — use `Option<T>` for LLM-generated fields
