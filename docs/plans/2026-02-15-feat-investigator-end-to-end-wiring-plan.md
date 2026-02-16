---
title: "feat: Investigator end-to-end wiring + admin UI"
type: feat
date: 2026-02-15
---

# Investigator End-to-End Wiring + Admin UI

## Overview

Wire the existing investigation pipeline end-to-end so an admin can trigger signal investigations from the UI, watch them execute via Restate durable workflows, and review results (findings, evidence, tool call timeline, validation) in a full investigation detail view.

## Problem Statement

The investigation domain logic is built — 7 agent tools, adversarial validation, embedding dedup, connection graph creation — but it cannot be triggered or observed. The `triggerInvestigation` GraphQL mutation references a `WhyInvestigationWorkflow` that doesn't exist in Restate. The admin app has no way to trigger, monitor, or review investigations. The GraphQL schema is out of sync with the backend.

## Proposed Solution

Three workstreams executed in order (backend → schema → frontend):

1. **Backend**: Create and register two Restate workflows, add guard logic, expose investigation data via GraphQL
2. **Schema**: Regenerate `schema.graphql`, run codegen
3. **Frontend**: Build trigger UI, pending queue, and investigation detail page

## Technical Approach

### Phase 1: Backend — Restate Workflows + GraphQL Layer

#### 1.1 Create `WhyInvestigationWorkflow`

**New file:** `modules/rootsignal-domains/src/findings/restate/mod.rs`

Follow the pattern from `modules/rootsignal-domains/src/investigations/restate/mod.rs`:

```rust
// Request/Response structs
pub struct WhyInvestigateRequest {
    pub signal_id: String,
}
impl_restate_serde!(WhyInvestigateRequest);

pub struct WhyInvestigateResult {
    pub investigation_id: String,
    pub status: String,
    pub finding_id: Option<String>,
}
impl_restate_serde!(WhyInvestigateResult);

// Workflow trait
#[restate_sdk::workflow]
#[name = "WhyInvestigationWorkflow"]
pub trait WhyInvestigationWorkflow {
    async fn run(req: WhyInvestigateRequest) -> Result<WhyInvestigateResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

// Implementation
pub struct WhyInvestigationWorkflowImpl { deps: Arc<ServerDeps> }
// run() calls run_why_investigation() inside ctx.run(|| async move { ... })
```

- Accepts `{ signal_id }`, parses UUID, calls `run_why_investigation(InvestigationTrigger::FlaggedSignal { signal_id }, &deps)`
- Sets Restate state `"status"` for polling via `get_status`
- Returns `{ investigation_id, status, finding_id }`

**Also update:** `modules/rootsignal-domains/src/findings/mod.rs` to export the new `restate` submodule.

#### 1.2 Create `ClusterDetectionWorkflow`

**Same file** or sibling in `modules/rootsignal-domains/src/findings/restate/`:

```rust
#[restate_sdk::workflow]
#[name = "ClusterDetectionWorkflow"]
pub trait ClusterDetectionWorkflow {
    async fn run(req: EmptyRequest) -> Result<ClusterDetectionResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}
```

- Calls `detect_signal_clusters(&deps)` inside `ctx.run()`
- `detect_signal_clusters` already calls `run_why_investigation()` per cluster internally
- Single `ctx.run()` block is acceptable for now — clusters are detected in one pass, investigations fan out inline. We can refactor to per-cluster Restate calls later if fault isolation becomes important.

#### 1.3 Register Workflows in `main.rs`

**File:** `modules/rootsignal-server/src/main.rs`

Add to the Restate endpoint builder chain (around line 190):

```rust
.bind(WhyInvestigationWorkflowImpl::with_deps(worker_deps.clone()).serve())
.bind(ClusterDetectionWorkflowImpl::with_deps(worker_deps.clone()).serve())
```

Add the corresponding `use` imports for the workflow traits.

#### 1.4 Add Guard Logic to `triggerInvestigation` Mutation

**File:** `modules/rootsignal-server/src/graphql/findings/mutations.rs`

Before setting `investigation_status = 'pending'`, query current status:

```rust
let current = sqlx::query_scalar::<_, Option<String>>(
    "SELECT investigation_status FROM signals WHERE id = $1"
).bind(signal_id).fetch_one(pool).await?;

if current.as_deref() == Some("in_progress") {
    return Err(async_graphql::Error::new("Investigation already in progress"));
}
```

Allow re-triggering from `completed`, `linked`, or `null` (resets to `pending`).

#### 1.5 Create GraphQL Types for Investigation + Steps

**New file:** `modules/rootsignal-server/src/graphql/investigations/types.rs`

```rust
#[derive(SimpleObject)]
pub struct GqlInvestigation {
    pub id: ID,
    pub subject_type: String,
    pub subject_id: ID,
    pub trigger: String,
    pub status: String,
    pub summary: Option<String>,
    pub summary_confidence: Option<f32>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// With resolvers for nested data:
#[Object]
impl GqlInvestigation {
    async fn steps(&self, ctx: &Context<'_>) -> Result<Vec<GqlInvestigationStep>> { ... }
    async fn finding(&self, ctx: &Context<'_>) -> Result<Option<GqlFinding>> { ... }
    async fn signal(&self, ctx: &Context<'_>) -> Result<Option<GqlSignal>> { ... }
}

#[derive(SimpleObject)]
pub struct GqlInvestigationStep {
    pub id: ID,
    pub step_number: i32,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub page_snapshot_id: Option<ID>,
    pub created_at: DateTime<Utc>,
}
```

**New file:** `modules/rootsignal-server/src/graphql/investigations/mod.rs`

```rust
pub struct InvestigationQuery;

#[Object]
impl InvestigationQuery {
    async fn investigation(&self, ctx, id: ID) -> Result<Option<GqlInvestigation>> { ... }
    async fn investigations(&self, ctx, status: Option<String>, limit: i32, offset: i32) -> Result<Vec<GqlInvestigation>> { ... }
}
```

Register `InvestigationQuery` into `QueryRoot` via `MergedObject` in `modules/rootsignal-server/src/graphql/mod.rs`.

#### 1.6 Expose Investigation Fields on `GqlSignal`

**File:** `modules/rootsignal-server/src/graphql/signals/types.rs`

Add to `GqlSignal`:

```rust
pub needs_investigation: bool,
pub investigation_status: Option<String>,
pub investigation_reason: Option<String>,
```

Update the `From<Signal>` impl to populate these fields.

#### 1.7 Add Investigation Status Filter to Signals Query

**File:** `modules/rootsignal-server/src/graphql/signals/mod.rs`

Add `investigation_status: Option<String>` parameter to the `signals` query. When provided, filter with `WHERE investigation_status = $N`.

#### 1.8 Update `activeWorkflows` Query

**File:** `modules/rootsignal-server/src/graphql/workflows/mod.rs`

In the `active_workflows` function (around line 246), update the Restate admin API filter to include `WhyInvestigationWorkflow` and `ClusterDetectionWorkflow` alongside `ScrapeWorkflow`.

### Phase 2: Schema Regeneration + Codegen

#### 2.1 Regenerate `schema.graphql`

```bash
cargo run --bin export-schema modules/api-client-js/schema.graphql
```

#### 2.2 Run JS Codegen

```bash
cd modules/api-client-js && pnpm codegen
```

Verify the generated types include `GqlInvestigation`, `GqlInvestigationStep`, and the new signal fields.

### Phase 3: Frontend — Admin UI

#### 3.1 "Investigate" Button on Signal Detail Page

**File:** `modules/admin-app/app/(app)/signals/[id]/page.tsx`

Add a client component `InvestigateButton` (similar to the `DetectEntityButton` pattern in `sources/[id]/run-button.tsx`):

- Shows current `investigation_status` as badge
- "Investigate" button visible when status is `null`, `completed`, or `linked`
- Hidden/disabled when `in_progress`
- On click: calls `triggerInvestigation(signalId)` mutation
- Shows loading state, then updates badge optimistically
- If investigation exists (status = `completed`), shows link to `/investigations/[id]`

#### 3.2 Investigation Status Badge on Signals List

**File:** `modules/admin-app/app/(app)/signals/page.tsx`

Add `investigationStatus` to the signals list query. Add a column or badge showing:

| Status | Badge |
|--------|-------|
| `null` | (none) |
| `pending` | `bg-yellow-100 text-yellow-800` "Pending" |
| `in_progress` | `bg-blue-100 text-blue-800` "Investigating" |
| `completed` | `bg-green-100 text-green-800` "Complete" |
| `linked` | `bg-purple-100 text-purple-800` "Linked" |

Add row-level "Investigate" action button (small icon button in action column).

#### 3.3 Investigations List / Pending Queue Page

**New file:** `modules/admin-app/app/(app)/investigations/page.tsx`

Follow the established list page pattern (`findings/page.tsx`):

- Status filter tabs: All, Pending, Running, Completed, Failed
- Table columns: Status, Trigger, Subject (signal link), Summary (truncated), Confidence, Started, Duration
- Pagination with offset
- The "Pending" tab effectively serves as the Pending Investigations Queue
- For signals with `needs_investigation=true` but no Investigation record yet: query via `signals(investigationStatus: "pending")` and show as "Awaiting trigger" rows with an "Investigate" button

#### 3.4 Investigation Detail Page

**New file:** `modules/admin-app/app/(app)/investigations/[id]/page.tsx`

Layout (follows Finding detail page pattern):

```
← Back to Investigations

[Status Badge] [Validation Badge]       [Re-investigate button]
Investigation #abc123

┌─ Metadata ──────────────────────────────────────────┐
│ Trigger: flagged_signal:uuid-here                   │
│ Signal: [link to /signals/uuid]                     │
│ Status: completed                                   │
│ Confidence: 0.85                                    │
│ Started: 2026-02-15 14:30 | Duration: 3m 42s       │
└─────────────────────────────────────────────────────┘

┌─ Summary ───────────────────────────────────────────┐
│ [investigation.summary text]                        │
└─────────────────────────────────────────────────────┘

┌─ Tool Call Timeline ────────────────────────────────┐
│ Step 1: follow_link                                 │
│   Input: { url: "https://..." }                     │
│   Output: { title: "...", content_preview: "..." }  │
│   [View snapshot →]                                 │
│                                                     │
│ Step 2: query_signals                               │
│   Input: { city: "Minneapolis", type: "ask" }       │
│   Output: { count: 7, signals: [...] }              │
│                                                     │
│ Step 3: web_search                                  │
│   Input: { query: "ICE enforcement Twin Cities" }   │
│   Output: { result_count: 5, results: [...] }       │
│ ...                                                 │
└─────────────────────────────────────────────────────┘

┌─ Validation Results ────────────────────────────────┐
│ Quote Checks: 4/4 passed ✓                          │
│ Counter-hypothesis: "Seasonal food bank increase"   │
│ Simpler explanation likely: No                      │
│ Sufficient sources: Yes (4 sources)                 │
│ Sufficient evidence types: Yes (3 types)            │
│ Scope proportional: Yes                             │
│ Result: ACCEPTED                                    │
│ Reasoning: [validator reasoning text]               │
└─────────────────────────────────────────────────────┘

┌─ Resulting Finding ─────────────────────────────────┐
│ [Link to /findings/uuid]                            │
│ Title: "ICE enforcement operations in Twin Cities"  │
│ Status: published | Evidence: 6 items               │
└─────────────────────────────────────────────────────┘

┌─ Connections ───────────────────────────────────────┐
│ evidence_of: Signal "rent relief" → Finding         │
│ evidence_of: Signal "grocery delivery" → Finding    │
│ driven_by: Finding → "Federal enforcement order"    │
│ (each with causal_quote and confidence)             │
└─────────────────────────────────────────────────────┘
```

Steps are rendered as expandable cards. Input/output shown as formatted JSON. Page snapshot links open the snapshot URL.

#### 3.5 Add "Investigations" to Sidebar Nav

**File:** `modules/admin-app/app/(app)/layout.tsx`

Add to `NAV_ITEMS` array after "Findings":

```typescript
{ href: "/investigations", label: "Investigations" },
```

#### 3.6 Link Finding Detail → Investigation

**File:** `modules/admin-app/app/(app)/findings/[id]/page.tsx`

In the Provenance section, add a link to `/investigations/[investigationId]` when `investigationId` is present.

## Acceptance Criteria

### Functional Requirements

- [ ] Admin can click "Investigate" on a signal detail page and see the workflow trigger successfully
- [ ] Admin can click "Investigate" on a signal list row
- [ ] Triggering investigation on an `in_progress` signal returns an error
- [ ] Investigation status badge appears on signal list rows and detail pages
- [ ] Investigations list page shows all investigations with status filter tabs
- [ ] Pending queue tab shows signals awaiting investigation with trigger buttons
- [ ] Investigation detail page shows metadata, summary, confidence
- [ ] Investigation detail page shows step-by-step tool call timeline with inputs/outputs
- [ ] Investigation detail page shows validation results (quote checks, counter-hypothesis, sufficiency)
- [ ] Investigation detail page links to resulting Finding (or shows "Rejected" if validation failed)
- [ ] Investigation detail page shows connections with causal quotes
- [ ] Finding detail page links back to its Investigation
- [ ] Cluster detection can be triggered and creates investigations
- [ ] `schema.graphql` is in sync with backend types

### Non-Functional Requirements

- [ ] Investigation execution does not block the GraphQL request (async via Restate)
- [ ] Restate retries failed investigations (with eventual `failed` terminal state)

## File Change Summary

### New Files

| File | Purpose |
|------|---------|
| `modules/rootsignal-domains/src/findings/restate/mod.rs` | WhyInvestigationWorkflow + ClusterDetectionWorkflow |
| `modules/rootsignal-server/src/graphql/investigations/mod.rs` | Investigation GraphQL queries |
| `modules/rootsignal-server/src/graphql/investigations/types.rs` | GqlInvestigation, GqlInvestigationStep types |
| `modules/admin-app/app/(app)/investigations/page.tsx` | Investigations list + pending queue |
| `modules/admin-app/app/(app)/investigations/[id]/page.tsx` | Investigation detail page |

### Modified Files

| File | Change |
|------|--------|
| `modules/rootsignal-domains/src/findings/mod.rs` | Export `restate` submodule |
| `modules/rootsignal-server/src/main.rs` | Register WhyInvestigationWorkflow + ClusterDetectionWorkflow |
| `modules/rootsignal-server/src/graphql/mod.rs` | Add InvestigationQuery to QueryRoot |
| `modules/rootsignal-server/src/graphql/signals/types.rs` | Add investigation fields to GqlSignal |
| `modules/rootsignal-server/src/graphql/signals/mod.rs` | Add investigation_status filter |
| `modules/rootsignal-server/src/graphql/findings/mutations.rs` | Add guard logic to triggerInvestigation |
| `modules/rootsignal-server/src/graphql/workflows/mod.rs` | Add investigation workflows to activeWorkflows filter |
| `modules/admin-app/app/(app)/layout.tsx` | Add "Investigations" to sidebar nav |
| `modules/admin-app/app/(app)/signals/[id]/page.tsx` | Add InvestigateButton + status badge |
| `modules/admin-app/app/(app)/signals/page.tsx` | Add investigation_status badge + action button |
| `modules/admin-app/app/(app)/findings/[id]/page.tsx` | Add investigation link in Provenance section |
| `modules/api-client-js/schema.graphql` | Regenerated |

## Design Decisions

- **WhyInvestigationWorkflow is separate from InvestigateWorkflow**: They serve different purposes (signal-based findings vs. entity-based entity investigations). Keep both.
- **ClusterDetectionWorkflow runs inline**: Detection + per-cluster investigations in a single `ctx.run()`. Simpler now; can refactor to per-cluster Restate fan-out later if fault isolation matters.
- **Connection graph starts as a flat list**: Reuse the existing Finding detail pattern for showing connections as a list with role/quote/confidence. Full interactive graph (d3/react-flow) deferred.
- **No real-time streaming of steps**: Poll investigation status and steps on a 5-second interval when status is `running`. SSE/WebSocket deferred.
- **Validation results stored on Investigation**: The adversarial validation output should be stored as JSON on the Investigation record (or a related validation_result record) so the detail page can display it.

## Open Questions

- Where should validation results be persisted? Options: JSON column on `investigations` table, separate `validation_results` table, or stored in the last `investigation_step`.
- Should the pending queue page poll for new pending signals, or is manual refresh sufficient for v1?

## References

- Brainstorm: `docs/brainstorms/2026-02-15-investigator-end-to-end-wiring-brainstorm.md`
- Existing Restate pattern: `modules/rootsignal-domains/src/investigations/restate/mod.rs`
- Investigation logic: `modules/rootsignal-domains/src/findings/activities/investigate.rs`
- Admin UI patterns: `modules/admin-app/app/(app)/findings/page.tsx`, `findings/[id]/page.tsx`
- GraphQL architecture: `docs/plans/2026-02-14-feat-graphql-api-plan.md`
