---
date: 2026-02-15
topic: investigator-end-to-end-wiring
---

# Investigator End-to-End Wiring + Admin UI

## What We're Building

Wire the existing investigation pipeline end-to-end so an admin can trigger investigations from the UI, watch them execute, and review the results. Three workstreams:

### Workstream 1: Fix Restate Wiring (Backend)
- Create and register `WhyInvestigationWorkflow` that calls `run_why_investigation()`
- Wire `ClusterDetectionWorkflow` to call `detect_signal_clusters()`
- Wire `InvestigationStep::create()` into all 7 investigation tools so the audit trail is recorded
- Regenerate `schema.graphql` to include Finding types/queries/mutations

### Workstream 2: Investigation Trigger UI
- Add "Investigate" button on signal detail pages
- Add action button in signals list table rows
- Add a **Pending Investigations Queue** — dedicated page showing signals where `needs_investigation=true` and `investigation_status='pending'`, with bulk "Investigate" actions
- Show `investigation_status` badge on signal cards/rows

### Workstream 3: Investigation Detail View
- New `/investigations/[id]` page showing:
  - Investigation metadata (trigger, status, timing, confidence)
  - Step-by-step tool call timeline — each `InvestigationStep` rendered as a card showing tool name, input, output, and any snapshotted page
  - Evidence trail — linked `FindingEvidence` records with quotes and attribution
  - Validation results — the adversarial validation output (quote checks, counter-hypothesis, sufficiency)
  - Resulting Finding — link to the Finding created (or "rejected" if validation failed)
  - Connection graph — visual showing signals → finding → evidence relationships
- Add "Investigations" to sidebar nav
- Link from Finding detail page back to its Investigation

## Why This Approach

The investigation logic is already solid — 7 tools, adversarial validation, embedding dedup. The gap is purely in the plumbing (Restate workflows) and visibility (admin UI). Fixing these makes the system usable without changing the core algorithm.

## Key Decisions
- **Manual trigger first**: Gives control over cost and quality before enabling automatic triggers
- **Restate for durability**: Investigations are multi-minute, multi-LLM-call operations
- **Full investigation view**: The step-by-step audit trail is critical for understanding and tuning the system
- **Record investigation steps**: Without this, the investigation is a black box

## Open Questions
- Should the pending queue auto-refresh or poll for status updates?
- Should investigation steps stream to the UI in real-time (SSE/WebSocket) or only show after completion?
- What's the investigation timeout? (Currently unbounded — should cap at ~5 minutes?)

## Next Steps
→ `/workflows:plan` for implementation details
