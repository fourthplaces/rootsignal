---
title: "feat: Signal review status badges in admin app"
type: feat
date: 2026-02-24
---

# Signal Review Status Badges in Admin App

## Overview

Surface the signal lifecycle as colored badges in the admin app, so operators can see at a glance whether each signal is staged, published, corrected, or rejected. Persist correction metadata and rejection reasons on Neo4j nodes. Rename `quarantined` → `rejected` for clarity.

## Problem Statement

Signals flow through a lifecycle (`staged` → `live` / `quarantined`) but none of this is visible in the admin app. `review_status` is a backend-only Neo4j property. The admin can't distinguish a cleanly-passed signal from one the linter auto-corrected, or see why a signal was rejected. The term "quarantined" is jargon — "rejected" is clearer.

## Proposed Solution

### Badge taxonomy

| `review_status` | `was_corrected` | Badge | Color |
|---|---|---|---|
| `staged` | — | **Staged** | Amber (`bg-amber-500/10 text-amber-400 border-amber-500/20`) |
| `live` | `false`/`null` | **Published** | Green (`bg-green-500/10 text-green-400 border-green-500/20`) |
| `live` | `true` | **Corrected** | Blue (`bg-blue-500/10 text-blue-400 border-blue-500/20`) |
| `rejected` | — | **Rejected** | Red (`bg-red-500/10 text-red-400 border-red-500/20`) |

### New Neo4j properties on signal nodes

- `was_corrected: bool` — `true` when linter auto-fixed fields before promoting
- `corrections: String` — JSON array: `[{"field": "title", "from": "old", "to": "new"}]`
- `rejection_reason: String` — free-form text from linter or supervisor

### Admin sees all statuses

Add an admin-specific GraphQL query that does not filter on `review_status = 'live'`, behind `AdminGuard`. Multi-select filter dropdown for status.

## Key Decisions

- **Merge `quarantined` → `rejected`**: Both linter and supervisor rejections share one status. `rejection_reason` text naturally indicates the source.
- **`was_corrected` flag (option B)**: Separate bool rather than a new `review_status` value. Downstream consumers filtering `review_status = 'live'` need no changes.
- **Corrections stored on Neo4j node**: Duplicates Postgres run log data but avoids cross-database joins in GraphQL resolvers. Acceptable tradeoff.
- **JSON string for `corrections`**: Neo4j doesn't support nested objects. Store as serialized JSON string, parse client-side.
- **Staged signals are view-only**: No manual approve/reject buttons for now.
- **Legacy signals**: Existing `live` signals with `was_corrected = null` treated as Published (green). Backfill sets `was_corrected = false`.

## Acceptance Criteria

- [x] `quarantined` renamed to `rejected` across Rust codebase (enum, tool, prompt, events)
- [x] Neo4j migration renames `quarantined` → `rejected` on all signal labels
- [x] Linter persists `was_corrected`, `corrections` JSON, and `rejection_reason` on signal nodes
- [x] Supervisor persists `rejection_reason` on signal nodes
- [x] `NodeMeta` struct includes `review_status`, `was_corrected`, `corrections`, `rejection_reason`
- [x] GraphQL exposes these four fields on all signal types
- [x] Admin-specific query returns signals of all statuses with optional status filter
- [x] Admin signals list shows colored status badge per signal
- [x] Admin signal detail page shows rejection reason (if rejected) or corrections diff (if corrected)
- [x] Multi-select filter dropdown on signals list to filter by review status

## Implementation Phases

### Phase 1: Rename `quarantined` → `rejected` (Rust + Neo4j)

**Files:**

#### `modules/rootsignal-scout/src/pipeline/lint_tools.rs`
- Rename `LintVerdict::Quarantine { reason }` → `LintVerdict::Reject { reason }`
- Rename `QuarantineSignalTool` → `RejectSignalTool`
- Rename `QuarantineSignalArgs` → `RejectSignalArgs`
- Update tool name string from `"quarantine_signal"` to `"reject_signal"`
- Update tool description

#### `modules/rootsignal-scout/src/pipeline/signal_lint.rs`
- Update system prompt: replace "quarantine" language with "reject"
- Update verdict matching: `Quarantine` → `Reject`
- Update fallback quarantine paths (lines ~164, ~215) to use `"rejected"`
- Rename `EventKind::LintQuarantine` → `EventKind::LintReject` (or `LintRejection`)

#### `modules/rootsignal-graph/src/writer.rs`
- `set_review_status` calls that pass `"quarantined"` → `"rejected"`
- `StagedSignal` doc comment if it mentions quarantine

#### `modules/rootsignal-scout-supervisor/src/checks/batch_review.rs`
- Already uses `"rejected"` — verify no `"quarantined"` references remain

#### `modules/rootsignal-graph/src/migrate.rs`
- Add idempotent migration:

```cypher
-- For each signal label (Gathering, Aid, Need, Notice, Tension) + Story:
MATCH (n:{Label}) WHERE n.review_status = 'quarantined'
SET n.review_status = 'rejected'
RETURN count(n) AS updated
```

#### Test files
- Rename any test functions/assertions referencing quarantine → reject
- Test names should describe behavior: e.g., `linter_rejects_hallucinated_signal`

### Phase 2: Persist correction metadata + rejection reason

**Files:**

#### `modules/rootsignal-graph/src/writer.rs`

Add new method `set_signal_corrected`:

```rust
pub async fn set_signal_corrected(
    &self,
    signal_id: Uuid,
    corrections: &[FieldCorrection],
) -> Result<(), neo4rs::Error> {
    let json = serde_json::to_string(corrections)?;
    let labels = ["Gathering", "Aid", "Need", "Notice", "Tension"];
    for label in &labels {
        let cypher = format!(
            "MATCH (n:{label}) WHERE n.id = $id
             SET n.was_corrected = true, n.corrections = $corrections"
        );
        self.client.graph.run(
            query(&cypher)
                .param("id", signal_id.to_string())
                .param("corrections", json.clone())
        ).await?;
    }
    Ok(())
}
```

Update `set_review_status` to accept optional `reason`:

```rust
pub async fn set_review_status(
    &self,
    signal_id: Uuid,
    status: &str,
    reason: Option<&str>,
) -> Result<(), neo4rs::Error>
```

When `reason` is `Some`, also SET `n.rejection_reason = $reason`.

#### `modules/rootsignal-scout/src/pipeline/signal_lint.rs`

After applying corrections (`update_signal_fields`), call `set_signal_corrected` with the corrections vec before setting status to `live`.

For rejections, pass `reason` through to `set_review_status(id, "rejected", Some(&reason))`.

#### `modules/rootsignal-scout-supervisor/src/checks/batch_review.rs`

Pass rejection reason through to `set_review_status(id, "rejected", Some(&reason))`.

#### `modules/rootsignal-graph/src/migrate.rs`

Add backfill for existing signals:

```cypher
-- Backfill was_corrected = false on all signals where it's missing
MATCH (n:{Label}) WHERE n.was_corrected IS NULL
SET n.was_corrected = false
RETURN count(n) AS updated
```

### Phase 3: Expose via GraphQL

**Files:**

#### `modules/rootsignal-common/src/types.rs`

Add to `NodeMeta`:

```rust
pub review_status: String,
pub was_corrected: bool,
pub corrections: Option<String>,    // JSON string
pub rejection_reason: Option<String>,
```

#### `modules/rootsignal-graph/src/reader.rs`

- Update `NodeMeta` deserialization from Neo4j rows to read the four new fields
- Add new admin reader method `admin_signals_recent` that does NOT filter on `review_status = 'live'`
- Accept optional `review_statuses: Vec<String>` parameter for filtering

#### `modules/rootsignal-api/src/graphql/types.rs`

Add resolvers to the `signal_meta_resolvers!` macro (or inline on each type):

```rust
async fn review_status(&self) -> &str { &self.meta().review_status }
async fn was_corrected(&self) -> bool { self.meta().was_corrected }
async fn corrections(&self) -> Option<&str> { self.meta().corrections.as_deref() }
async fn rejection_reason(&self) -> Option<&str> { self.meta().rejection_reason.as_deref() }
```

#### `modules/rootsignal-api/src/graphql/schema.rs`

Add admin query:

```rust
#[graphql(guard = "AdminGuard")]
async fn admin_signals(
    &self,
    ctx: &Context<'_>,
    limit: Option<i32>,
    review_statuses: Option<Vec<String>>,
) -> Result<Vec<GqlSignal>>
```

### Phase 4: Admin app UI

**Files:**

#### `modules/admin-app/src/graphql/queries.ts`

Add `reviewStatus`, `wasCorrected`, `corrections`, `rejectionReason` to `SIGNAL_FIELDS` fragment.

Add `ADMIN_SIGNALS` query:

```graphql
query AdminSignals($limit: Int, $reviewStatuses: [String!]) {
  adminSignals(limit: $limit, reviewStatuses: $reviewStatuses) {
    ...SignalFields
  }
}
```

#### `modules/admin-app/src/components/ReviewStatusBadge.tsx` (new file)

```tsx
const STATUS_COLORS: Record<string, string> = {
  staged: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  published: "bg-green-500/10 text-green-400 border-green-500/20",
  corrected: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  rejected: "bg-red-500/10 text-red-400 border-red-500/20",
};

function deriveStatus(reviewStatus: string, wasCorrected: boolean): string {
  if (reviewStatus === "rejected") return "rejected";
  if (reviewStatus === "staged") return "staged";
  return wasCorrected ? "corrected" : "published";
}
```

Badge renders as:

```tsx
<span className={`inline-block px-2 py-0.5 rounded text-xs border ${STATUS_COLORS[status]}`}>
  {label}
</span>
```

Follows the existing pattern from `FindingsPage.tsx` severity badges.

#### `modules/admin-app/src/pages/SignalsPage.tsx`

- Switch from `SIGNALS_RECENT` to `ADMIN_SIGNALS` query
- Add "Status" column after "Type" in the table
- Add multi-select filter dropdown above the table for review statuses
- Default: show all statuses

#### `modules/admin-app/src/pages/SignalDetailPage.tsx`

- Show `ReviewStatusBadge` in the header area
- If `rejected`: show rejection reason in a red-tinted callout box
- If `corrected`: show corrections as a simple table (Field | Before | After) in a blue-tinted section

## References

- Brainstorm: `docs/brainstorms/2026-02-24-signal-review-status-badges-brainstorm.md`
- Signal lint plan: `docs/plans/2026-02-24-feat-signal-lint-plan.md`
- Existing badge patterns: `modules/admin-app/src/pages/FindingsPage.tsx:9-13` (severity colors)
- Writer review status: `modules/rootsignal-graph/src/writer.rs:6377` (`set_review_status`)
- Lint tools: `modules/rootsignal-scout/src/pipeline/lint_tools.rs:227` (`QuarantineSignalTool`)
- Migration patterns: `modules/rootsignal-graph/src/migrate.rs:1261` (backfill review status)
- Reader live filter: `modules/rootsignal-graph/src/reader.rs:61`
