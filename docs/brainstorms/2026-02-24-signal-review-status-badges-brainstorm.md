---
date: 2026-02-24
topic: signal-review-status-badges
---

# Signal Review Status Badges in Admin App

## What We're Building

Surface the signal lifecycle (`staged` → `live` / `rejected`) as visible badges in the admin app, so operators can see at a glance where each signal stands. Also persist correction metadata so corrected signals are distinguishable from clean passes.

## Current State

- Signals have `review_status` in Neo4j: `staged`, `live`, `quarantined`
- The linter transitions signals via three tools: `pass_signal`, `correct_signal`, `quarantine_signal`
- **None of this is visible in the admin app today** — `review_status` isn't exposed via GraphQL

## Key Decisions

### Badge taxonomy

| `review_status` | Badge | Color | Meaning |
|---|---|---|---|
| `staged` | Staged | Amber | Awaiting lint |
| `rejected` | Rejected | Red | Linter determined signal is fundamentally wrong |
| `live` | Published | Green | Passed lint, no corrections needed |
| `live` + `was_corrected` | Corrected | Blue | Passed lint, but fields were auto-fixed |

### Separate flag for corrections (option B)

Add `was_corrected: bool` on the signal node rather than a new `review_status` value. Keeps the lifecycle clean — anything `live` is live — and downstream consumers filtering on `review_status = 'live'` need no changes. The admin app checks both fields to decide the badge.

### Persist correction details (before/after)

Store `corrections: {field: {from: "old", to: "new"}}` on the node at lint time. Concrete and auditable — the diff speaks for itself. No need for AI reasoning text initially.

For rejected signals, persist the `reason` string from the linter (already exists in the `quarantine_signal` tool).

### Rename quarantined → rejected

- `quarantine_signal` tool → `reject_signal`
- `LintVerdict::Quarantine` → `LintVerdict::Reject`
- `review_status: 'quarantined'` → `review_status: 'rejected'`
- Migration needed for existing signals stored as `quarantined`

## Open Questions

- Should the signals list page show the badge inline in the table, or as a filterable column?
- Do we want a filter/tab to show only staged or only rejected signals?

## Next Steps

1. Rename `quarantined` → `rejected` across linter codebase + migration
2. Add `was_corrected` flag and `corrections` JSON to signal nodes at lint time
3. Expose `review_status`, `was_corrected`, `corrections`, `rejection_reason` via GraphQL
4. Add badge component to admin app signals list and detail pages
