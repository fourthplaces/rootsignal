---
date: 2026-02-24
topic: schedule-node
---

# ScheduleNode: Capturing Event & Signal Schedules

## What We're Building

A standalone `ScheduleNode` in the graph that captures schedule and recurrence information for any signal type. Rather than embedding schedule fields on individual signal types, a separate node connects to any signal (gathering, aid, need, notice, tension) via a `HAS_SCHEDULE` edge. The LLM extractor emits schedule data as a structured node; the graph wiring handles the rest.

## Why This Approach

The current model has `starts_at`, `ends_at`, and `is_recurring` on GatheringNode only. This is too limited — a food pantry (AidNode) open every Thursday, or a recurring town hall (GatheringNode) with exceptions, can't be expressed. Embedding richer schedule fields on every signal type would be repetitive and inflexible. A standalone graph node keeps the architecture consistent and lets any signal type gain schedule data without schema changes.

## Key Decisions

- **Standalone node in Neo4j**: Consistent with the existing graph architecture where signals, actors, places, and stories are all first-class nodes with edges between them.
- **Three tiers of schedule fidelity**:
  1. Structured RRULE (RFC 5545) when the LLM can extract a recurrence pattern
  2. Explicit date lists (`rdates`) for irregular schedules that don't fit a pattern
  3. Raw natural-language text as a fallback when neither is possible
- **Recurrence expansion in Rust at query time**: Neo4j stores the rule; the API layer materializes concrete dates for the UI. This keeps the graph clean and puts computation where it belongs.
- **`starts_at`/`ends_at` on GatheringNode stays**: Denormalized convenience for simple one-off events. ScheduleNode handles anything richer.

## ScheduleNode Fields

| Field | Type | Purpose |
|-------|------|---------|
| `id` | `UUID` | Standard node ID |
| `rrule` | `Option<String>` | RFC 5545 RRULE string (e.g., `FREQ=WEEKLY;BYDAY=TU,TH`) |
| `rdates` | `Vec<DateTime<Utc>>` | Explicit dates for irregular schedules |
| `exdates` | `Vec<DateTime<Utc>>` | Exception dates to exclude from recurrence |
| `dtstart` | `Option<DateTime<Utc>>` | Anchor start time for the recurrence |
| `dtend` | `Option<DateTime<Utc>>` | End time / duration anchor |
| `timezone` | `Option<String>` | IANA timezone (e.g., `America/Chicago`) |
| `schedule_text` | `Option<String>` | Raw natural-language fallback (e.g., "First Saturdays, rain or shine") |
| `extracted_at` | `DateTime<Utc>` | When this schedule was extracted |

## Edge

`(Signal)-[:HAS_SCHEDULE]->(ScheduleNode)` — any signal type can have zero or more schedule nodes.

## Open Questions

- **RRULE expansion library for Rust**: Need to evaluate options (e.g., `rrule` crate) for expanding recurrence rules into concrete date instances at query time.
- **Extraction prompt design**: How to coach the LLM to emit structured RRULE when possible, fall back to `rdates` for irregular patterns, and always preserve `schedule_text`.
- **UI calendar component**: What calendar/schedule UI component to use for rendering materialized dates.
- **Timezone resolution**: When the source page doesn't state a timezone, should we infer from the signal's `about_location`?

## Next Steps

-> `/workflows:plan` for implementation details
