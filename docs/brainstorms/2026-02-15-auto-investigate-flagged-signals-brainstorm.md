---
date: 2026-02-15
topic: auto-investigate-flagged-signals
---

# Auto-Investigate Flagged Signals

## What We're Building

Close the autonomous investigation loop by auto-triggering WhyInvestigationWorkflow when signal extraction flags a signal with `needs_investigation = true`. Today the flag gets set but nothing picks it up — signals sit in limbo until manually triggered from the admin UI.

With this change the full flywheel runs autonomously: **source → scrape → extract signals → auto-investigate → recommend new sources → scrape**.

## Why This Approach

All the infrastructure already exists:
- Signal extraction LLM already sets `needs_investigation = true` + `investigation_reason` during extraction (`extract_signals.rs:221`)
- `WhyInvestigationWorkflow` is registered in Restate and works end-to-end
- `process_source_recommendations` already inserts agent-recommended URLs into the sources table

The only missing piece is ~10 lines: trigger the workflow after flagging, with a concurrency guard.

## Key Decisions

- **Trigger point**: Right after `needs_investigation` flag is set in `extract_signals.rs`, not a separate polling job. Simplest path, no new infrastructure.
- **Rate limit**: Max 5 concurrent investigations (`SELECT COUNT(*) FROM signals WHERE investigation_status = 'in_progress'`). If at capacity, skip — signal stays flagged for manual trigger or later pickup.
- **No approval gate**: Fully autonomous. The investigation prompt already has guardrails (grounded evidence, adversarial validation, dedup). Admin UI provides visibility.

## Open Questions

- Should there be a periodic sweep job that picks up flagged signals that were skipped due to rate limits? (Can defer — manual trigger covers this for now)
- Should cluster detection also auto-trigger? (Separate decision, same pattern)

## Next Steps

→ `/workflows:plan` for implementation details
