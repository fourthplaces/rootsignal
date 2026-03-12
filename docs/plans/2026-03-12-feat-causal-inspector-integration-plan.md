---
title: "feat: Integrate causal_inspector into API and admin app"
type: feat
date: 2026-03-12
---

# Causal Inspector Integration

## Overview

Replace the custom events page infrastructure (5 panes, hand-rolled GraphQL queries, custom WebSocket bridge) with the `causal_inspector` crate's built-in GraphQL + WebSocket router on the backend, and the `causal-inspector` npm package on the frontend.

This gives us reactor observability (logs, outcomes, descriptions), aggregate lifecycle views, correlation summaries, and dependency graphs — all for free from the library.

## Problem Statement

The current events page is a hand-built read model with:
- Custom GraphQL queries (`list_events_paginated`, `causal_tree`, `causal_flow`)
- Custom `EventBroadcast` → `AdminEvent` → GraphQL subscription pipeline
- 100+ lines of `event_summary()` formatting in `db/models/scout_run.rs`
- No visibility into reactor execution (logs, outcomes, retries, DLQ)
- No aggregate state timeline or lifecycle views
- No correlation summaries or dependency graphs

The `causal_inspector` crate provides all of this via a standard `InspectorReadModel` trait + `EventDisplay` trait + axum router.

## Proposed Solution

### Phase 1: Backend — Rust API server

#### 1a. Change causal_inspector feature flag

**File:** `Cargo.toml` (workspace root, line 93)

```toml
# Before
causal_inspector = { version = "0.1.2", features = ["graphql"] }

# After
causal_inspector = { version = "0.1.2", features = ["axum"] }
```

The `axum` feature enables `graphql` automatically and adds the `router()` function.

#### 1b. Create `RootsignalEventDisplay`

**New file:** `modules/rootsignal-api/src/inspector_display.rs`

Implement `EventDisplay` by wrapping existing functions:

```rust
use causal_inspector::EventDisplay;
use crate::db::models::scout_run::{event_summary, event_domain_prefix};

#[derive(Clone)]
pub struct RootsignalEventDisplay;

impl EventDisplay for RootsignalEventDisplay {
    fn display_name(&self, event_type: &str, _payload: &serde_json::Value) -> String {
        // event_type is "domain:variant_name" — humanize the variant
        let variant = event_type.split_once(':').map(|(_, v)| v).unwrap_or(event_type);
        let domain = event_domain_prefix(event_type);
        format!("{domain}:{variant}")
    }

    fn summary(&self, event_type: &str, payload: &serde_json::Value) -> Option<String> {
        let variant = event_type.split_once(':').map(|(_, v)| v).unwrap_or(event_type);
        event_summary(variant, payload)
    }
}
```

**Note:** `event_summary` and `event_domain_prefix` are currently `pub(crate)` — no visibility change needed.

#### 1c. Create `PgInspectorReadModel`

**New file:** `modules/rootsignal-api/src/inspector_read_model.rs`

Implement `InspectorReadModel` (17 methods) against the existing Postgres tables:

| Method | Table | Notes |
|--------|-------|-------|
| `list_events(query)` | `events` | Maps `EventQuery` → existing paginated query pattern |
| `get_event(seq)` | `events` | Simple `WHERE seq = $1` |
| `causal_tree(seq)` | `events` | Reuse existing `causal_tree()` logic (correlation_id lookup) |
| `causal_flow(correlation_id)` | `events` | `WHERE correlation_id = $1 ORDER BY seq` |
| `events_from_seq(start, limit)` | `events` | `WHERE seq >= $1 ORDER BY seq LIMIT $2` |
| `reactor_logs(event_id, reactor_id)` | `seesaw_handler_journal` | `WHERE handler_id = $2 AND event_id = $1` |
| `reactor_logs_by_correlation(cid)` | `seesaw_handler_journal` + `events` | Join via event_id to get correlation |
| `reactor_outcomes(cid)` | `seesaw_effect_executions` | `WHERE correlation_id = $1` |
| `reactor_descriptions(cid)` | — | Return empty vec (no description table yet) |
| `reactor_description_snapshots(cid)` | — | Return empty vec |
| `aggregate_state_timeline(cid)` | — | Return empty vec (no snapshot table yet) |
| `list_correlations(search, limit)` | `events` | Aggregate query: `GROUP BY correlation_id` |
| `reactor_dependencies()` | — | Return empty vec (static metadata, could be hardcoded later) |
| `aggregate_lifecycle(key, limit)` | — | Return empty vec |
| `list_aggregate_keys()` | `events` | `SELECT DISTINCT aggregate_type \|\| ':' \|\| aggregate_id` |

**Column mapping** (`events` → `StoredEvent`):

| events column | StoredEvent field |
|---------------|-------------------|
| `seq` | `seq` |
| `ts` | `ts` |
| `event_type` | `event_type` |
| `payload` | `payload` |
| `id` | `id` |
| `parent_id` | `parent_id` |
| `correlation_id` | `correlation_id` |
| `handler_id` | `reactor_id` |
| `aggregate_type` | `aggregate_type` |
| `aggregate_id` | `aggregate_id` |
| — | `stream_version` (None, not tracked) |

**Key implementation notes:**
- The `reactor_logs` method requires `seesaw_handler_journal` (migration 035). Journal entries use `(handler_id, event_id, seq)` as PK. Map `seq` → ordering, `value` → data+message.
- The `reactor_outcomes` method maps from `seesaw_effect_executions`: `status`, `error`, `attempts`, `created_at` → `started_at`, `updated_at` → `completed_at`.
- Methods without backing tables return empty vecs — the UI gracefully handles missing data.

#### 1d. Wire into main.rs — broadcast bridge

The inspector's `router()` needs a `broadcast::Sender<StoredEvent>` for live subscriptions. Bridge from the existing `EventBroadcast`:

```rust
// In main(), after EventBroadcast::spawn():
let (inspector_tx, _) = tokio::sync::broadcast::channel::<causal_inspector::StoredEvent>(1024);

if let Some(ref broadcast) = event_broadcast {
    let mut rx = broadcast.subscribe();
    let tx = inspector_tx.clone();
    tokio::spawn(async move {
        while let Ok(admin_event) = rx.recv().await {
            let stored = admin_event_to_stored_event(&admin_event);
            let _ = tx.send(stored);
        }
    });
}
```

This needs a conversion function `admin_event_to_stored_event` that maps `AdminEvent` → `StoredEvent`. The `AdminEvent` struct already contains seq, ts, event_type, payload, id, parent_id, correlation_id, handler_id — all the fields `StoredEvent` needs.

#### 1e. Wire into main.rs — nest the router

```rust
use causal_inspector;
use inspector_display::RootsignalEventDisplay;
use inspector_read_model::PgInspectorReadModel;

// In main(), after pg_pool is created:
let inspector_router = if let Some(ref pool) = pg_pool {
    let read_model = Arc::new(PgInspectorReadModel::new(pool.clone()));
    let display = RootsignalEventDisplay;
    Some(causal_inspector::router(read_model, display, inspector_tx.clone()))
} else {
    None
};

// In Router::new(), add:
let app = Router::new()
    // ... existing routes ...
    ;

// Nest inspector after building base app
let app = if let Some(inspector) = inspector_router {
    app.nest("/api/inspector", inspector)
} else {
    app
};
```

This mounts the inspector's GraphQL endpoint at `POST /api/inspector` and WebSocket at `GET /api/inspector/ws`.

#### 1f. Register module

Add to `main.rs`:
```rust
mod inspector_display;
mod inspector_read_model;
```

### Phase 2: Frontend — Admin app

#### 2a. Replace npm dependency

```bash
cd modules/admin-app
npm uninstall @causal/inspector-ui
npm install causal-inspector
```

#### 2b. Update EventsPage

Replace the entire custom events page with the inspector component:

**File:** `modules/admin-app/src/pages/events/EventsPage.tsx`

```tsx
import { CausalInspector } from "causal-inspector";

export function EventsPage() {
  return (
    <div className="h-[calc(100vh-3rem)] -m-6">
      <CausalInspector endpoint="/api/inspector" />
    </div>
  );
}
```

This replaces the 5-pane PaneManager + EventsPaneProvider + all custom pane components with the inspector's built-in UI (which provides timeline, causal tree, causal flow, reactor logs, aggregate lifecycle, and more).

#### 2c. Clean up unused files

The following files become dead code after the switch:
- `pages/events/EventsPaneContext.tsx`
- `pages/events/defaultLayout.ts`
- `pages/events/eventColor.ts`
- `pages/events/panes/TimelinePane.tsx`
- `pages/events/panes/CausalTreePane.tsx`
- `pages/events/panes/CausalFlowPane.tsx`
- `pages/events/panes/InvestigatePane.tsx`
- `pages/events/panes/LogsPane.tsx`

**Decision needed:** Delete these immediately, or keep for one release as reference?

## Acceptance Criteria

- [ ] `causal_inspector` feature changed from `"graphql"` to `"axum"` in workspace Cargo.toml
- [ ] `PgInspectorReadModel` implements all 17 `InspectorReadModel` methods
- [ ] `RootsignalEventDisplay` wraps existing `event_summary()` and `event_domain_prefix()`
- [ ] Inspector router nested at `/api/inspector` in main.rs
- [ ] Broadcast bridge converts `AdminEvent` → `StoredEvent` for live subscriptions
- [ ] `@causal/inspector-ui` replaced with `causal-inspector` npm package
- [ ] EventsPage uses `<CausalInspector endpoint="/api/inspector" />`
- [ ] Existing admin app routes (scout, signals, situations, etc.) unaffected
- [ ] Compiles and runs locally with `cargo build` + `npm run dev`

## Technical Considerations

### Existing events infrastructure stays
The custom GraphQL queries in `graphql/schema.rs` (events, causalTree, causalFlow) are still used by the ScoutRunDetailPage's event timeline. Don't delete them — the inspector is an addition, not a wholesale replacement of the events API.

### AdminEvent → StoredEvent conversion
`AdminEvent` (in `graphql/schema.rs`) needs inspection to map its fields. It's constructed from `EventRowFull` in `event_broadcast.rs` via `AdminEvent::from(row)`. The conversion to `StoredEvent` should be straightforward since both have the same source data.

### CORS
The inspector's GraphQL endpoint at `/api/inspector` inherits the same CORS layer as all other routes — no additional configuration needed.

### Auth
The inspector router is mounted inside the same axum app — it gets the same security headers. No JWT gating is applied to the inspector endpoint (same as the existing `/graphql` endpoint which relies on `AuthContext` at the resolver level). The inspector has no built-in auth — consider gating behind admin check later if needed.

### Graceful degradation
If `pg_pool` is `None` (no DATABASE_URL), the inspector router is not mounted. Same pattern as existing scout workflows.

## Build Order

1. **1a** — Feature flag change (1 line)
2. **1b** — `inspector_display.rs` (~20 lines)
3. **1c** — `inspector_read_model.rs` (~200 lines, bulk of the work)
4. **1d+1e+1f** — main.rs wiring (~30 lines)
5. **Verify** — `cargo build`, run locally, hit `/api/inspector` with GraphiQL
6. **2a** — npm dependency swap
7. **2b** — EventsPage replacement
8. **2c** — Dead code cleanup (optional, can defer)

## References

- `causal_inspector` 0.1.2 source: `~/.cargo/registry/src/.../causal_inspector-0.1.2/`
- `InspectorReadModel` trait: 17 async methods, all return `anyhow::Result<T>`
- `EventDisplay` trait: `display_name()` + `summary()`
- `router()` signature: `fn router<D: EventDisplay + Clone + 'static>(read_model: Arc<dyn InspectorReadModel>, display: D, event_tx: broadcast::Sender<StoredEvent>) -> Router`
- Existing event queries: `modules/rootsignal-api/src/db/models/scout_run.rs`
- Existing event broadcast: `modules/rootsignal-api/src/event_broadcast.rs`
- Admin app events page: `modules/admin-app/src/pages/events/EventsPage.tsx`
- Events table schema: migration 007 + 016 + 020 + 022 + 035
- Reactor tables: migration 023 (seesaw_effect_executions) + 035 (seesaw_handler_journal)
