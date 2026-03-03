---
title: "feat: Add SystemLog telemetry event"
type: feat
date: 2026-03-03
---

# Add SystemLog Telemetry Event

## Overview

Add a `SystemLog` variant to `TelemetryEvent` — a structured log line that participates in the causal chain via `parent_id` and `correlation_id`. These are contextual breadcrumbs explaining *why* other events are shaped the way they are (e.g., what went into an LLM before extraction, what template was chosen, what inputs were considered).

Not a play-by-play trace. Selectively emitted when something non-obvious happened or when capturing decision inputs.

Brainstorm: `docs/brainstorms/2026-03-03-system-log-event-brainstorm.md`

## Proposed Solution

Add a single variant to the existing `TelemetryEvent` enum:

```rust
SystemLog {
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
}
```

This is the lightest-touch approach:
- No new codec — `TelemetryEvent` already has codec registration, `event_layer()` already classifies it as telemetry
- No new enum — one variant, two fields
- Participates in causal chains automatically (seesaw assigns `parent_id` and `correlation_id` on emit)
- Shows up in the events browser immediately — variant name `system_log` displayed via `payload["type"]`

Add a stdout projection handler that prints `SystemLog` messages via `tracing::info!` as they're emitted.

## Acceptance Criteria

- [ ] `TelemetryEvent::SystemLog { message, context }` variant exists and serializes correctly
- [ ] `Eventlike` impl returns `"system_log"` for the new variant
- [ ] Stdout projection handler prints log messages via `tracing::info!` during engine runs
- [ ] `event_summary()` in schema.rs surfaces `message` field for events browser display
- [ ] At least one handler emits a `SystemLog` (extraction is the obvious first candidate)

## Implementation

### Step 1: Add variant to TelemetryEvent

**File:** `modules/rootsignal-common/src/telemetry_events.rs`

Add after `DemandAggregated` (line 90):

```rust
SystemLog {
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
},
```

Update `Eventlike` impl (line 108):

```rust
TelemetryEvent::SystemLog { .. } => "system_log",
```

### Step 2: Add stdout projection handler

**File:** `modules/rootsignal-scout/src/core/projection.rs`

New handler following the `on_any()` pattern used by `scout_runs_handler` and `capture_handler`:

```rust
pub fn system_log_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("system_log_stdout")
        .priority(2)
        .then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
            async move {
                if let Some(log) = event.downcast_ref::<TelemetryEvent>() {
                    if let TelemetryEvent::SystemLog { message, .. } = log {
                        tracing::info!(target: "system_log", "{}", message);
                    }
                }
                Ok(events![])
            }
        })
}
```

### Step 3: Register handler in engine builder

**File:** `modules/rootsignal-scout/src/core/engine.rs` (line ~148)

Add after `lifecycle::__seesaw_effect_scrape_finalize()`:

```rust
engine = engine.with_handler(projection::system_log_handler());
```

### Step 4: Surface `message` in events browser summary

**File:** `modules/rootsignal-api/src/graphql/schema.rs`

In `event_summary()` (line 1453), add `message` to the priority chain — insert before the existing `title` check since for `system_log` events, `message` is the primary display field:

```rust
fn event_summary(variant_name: &str, data: &serde_json::Value) -> Option<String> {
    json_str(data, "message")
        .or_else(|| json_str(data, "title"))
        // ... rest unchanged
```

### Step 5: Emit from a handler (first usage)

Pick the LLM extraction activity as the first callsite — this is the use case the user described ("what goes into an LLM before extraction").

**File:** `modules/rootsignal-scout/src/core/extractor.rs` (or whichever activity calls the LLM for extraction)

```rust
events = events.add(TelemetryEvent::SystemLog {
    message: format!("extracting signals from {} ({} chars, template={})", url, content.len(), template_name),
    context: Some(serde_json::json!({
        "url": url,
        "content_chars": content.len(),
        "template": template_name,
    })),
});
```

## Files to modify

| File | Change |
|------|--------|
| `modules/rootsignal-common/src/telemetry_events.rs` | Add `SystemLog` variant + `Eventlike` match arm |
| `modules/rootsignal-scout/src/core/projection.rs` | Add `system_log_handler()` |
| `modules/rootsignal-scout/src/core/engine.rs` | Register `system_log_handler()` in builder |
| `modules/rootsignal-api/src/graphql/schema.rs` | Add `message` to `event_summary()` priority chain |
| `modules/rootsignal-scout/src/core/extractor.rs` | First `SystemLog` emission site |

## Verification

1. `cargo check -p rootsignal-common` — variant compiles
2. `cargo check -p rootsignal-scout` — handler + emission compiles
3. `cargo check -p rootsignal-api` — summary change compiles
4. Run a scout locally, verify `system_log` entries appear in stdout via tracing
5. Open `/events` in admin app, verify `system_log` entries appear in timeline with message as summary
6. Click a `system_log` entry, verify it appears in the causal tree nested under its parent event

## References

- `modules/rootsignal-common/src/telemetry_events.rs:15-91` — existing TelemetryEvent enum
- `modules/rootsignal-scout/src/core/projection.rs:79-128` — `scout_runs_handler` pattern (on_any observer)
- `modules/rootsignal-api/src/graphql/schema.rs:1453-1481` — `event_summary()` helper
- `modules/rootsignal-scout/src/core/engine.rs:136-178` — engine builder handler registration
