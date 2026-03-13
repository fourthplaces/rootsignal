---
date: 2026-03-03
topic: system-log-event
---

# SystemLog Event Type

## What We're Building

A `SystemLog` event type — a structured log line that participates in the causal chain via `parent_id` and `correlation_id`. These are contextual breadcrumbs explaining *why* other events are shaped the way they are. For example, logging what goes into an LLM before extraction, what template was chosen, what inputs were considered.

These are not operational trace logs (TelemetryEvent already covers that). They selectively enrich the existing event stream with decision context — the "why" behind the output.

## Why This Approach

We considered several payload shapes:

- **Message + context bag** (chosen): minimal, low-friction, no taxonomy to bike-shed
- **Message + level/severity**: rejected — errors surface as their own events, and forcing callsites to choose info vs warn discourages logging
- **Message + category**: rejected — the causal chain already categorizes via parent_id; a log parented to `SignalsExtracted` is implicitly "about extraction"

The design optimizes for adoption. The moment you add required taxonomy to a log line, people stop emitting them. If filtering by severity is needed later, it's just a new optional field — no migration in an event store.

## Key Decisions

- **Taxonomy layer**: Telemetry. SystemLog is metadata *about* decisions, not a decision itself.
- **Payload shape**: `{ message: String, context: Option<serde_json::Value> }`. Message is human-readable, context is an optional structured bag for machine-readable details.
- **No level/category/severity fields**: the causal tree provides structure, the message provides meaning.
- **Selective emission**: handlers log when something non-obvious happened or when capturing decision inputs (e.g. LLM prompt context). Not a play-by-play trace.
- **Stdout projection**: an inline handler on the seesaw engine prints SystemLog messages to stdout as they're emitted. Zero-cost to add.
- **Events browser**: SystemLog events appear in the causal tree panel as leaf nodes under their parent event. The compact payload preview (`message` field) gives immediate context without expanding.

## Open Questions

- Should `context` be fully unstructured (serde_json::Value) or should we encourage a few conventional keys (e.g. `template`, `input_url`, `token_count`)? Leaning toward unstructured with conventions documented, not enforced.
- Should stdout projection include the parent event's name for context? e.g. `[signals:dedup] matched existing signal abc123 with 0.87 similarity`

## Next Steps

-> `/workflows:plan` for implementation details
