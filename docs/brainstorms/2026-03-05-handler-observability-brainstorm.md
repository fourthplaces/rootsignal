---
date: 2026-03-05
topic: handler-observability
---

# Handler Observability — ctx.logger

## What We're Building

A leveled logger on seesaw's `Context` that handlers call naturally during execution. Log entries persist atomically with handler completion and are queryable in the admin UI.

```rust
ctx.logger.info("fetched 12 URLs, 3 duplicates skipped");
ctx.logger.debug("raw LLM response", &llm_response);
ctx.logger.warn("embedding similarity below threshold: 0.42");
```

## Why This Approach

Today there is zero visibility into what handlers do between receiving an event and emitting output. The admin UI shows the event timeline — causal chains, payloads, handler skips/failures — but the "why" behind handler decisions is a black box. This serves three needs:

1. **Production debugging** — understand why a handler produced unexpected output
2. **Development visibility** — see what's happening as you iterate on handlers
3. **Operational monitoring** — spot slow, unhealthy, or badly-deciding handlers at a glance

## Key Decisions

- **Leveled logging**: `debug`, `info`, `warn` — mirrors standard log levels, familiar API
- **String message + optional structured data**: `ctx.logger.info("message")` for simple cases, `ctx.logger.info("message", &data)` when you need to attach JSON-serializable detail. Low friction by default, rich when needed.
- **Atomic persistence with handler completion**: Log entries attach to `HandlerCompletion` and commit in the same transaction as the handler result. No separate storage path, no orphaned logs. Tradeoff: logs from crashed (pre-completion) handler attempts are lost — acceptable because DLQ captures the error and `ctx.run()` journaling handles crash recovery.
- **UI deferred but data model ready**: Log entries must be queryable by `(correlation_id, event_id, handler_id)` so the admin GraphQL layer can serve them without schema redesign when we build the UI.

## Design Sketch

### Seesaw side (seesaw_core)

- Add a `Logger` struct to `Context` — internally a `Arc<Mutex<Vec<LogEntry>>>`
- `LogEntry`: `{ level: LogLevel, message: String, data: Option<serde_json::Value>, timestamp: DateTime<Utc> }`
- `LogLevel`: `Debug | Info | Warn`
- Engine drains the logger entries after handler execution and includes them in `HandlerCompletion`
- `HandlerCompletion` gets a new field: `log_entries: Vec<LogEntry>`
- Store implementations persist log entries alongside the handler result

### RootSignal side

- `PostgresStore` persists log entries (likely as JSONB array on the existing handler completion row, or a lightweight join table if we want per-entry indexing)
- Admin GraphQL exposes log entries per handler execution
- Admin UI: deferred, but the query surface is there

## Open Questions

- Should `ctx.logger.error()` exist, or should errors always flow through seesaw's error/DLQ path?
- Do we want a max log entry count or total payload size cap per handler execution to prevent runaway logging?
- Should debug-level entries be stripped in production builds / configurable per-handler?

## Next Steps

-> `/workflows:plan` for implementation details (seesaw changes first, then rootsignal wiring)
