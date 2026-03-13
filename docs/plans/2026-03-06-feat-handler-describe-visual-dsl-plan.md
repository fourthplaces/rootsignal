---
title: "feat: Handler describe visual DSL"
type: feat
date: 2026-03-06
---

# feat: Handler describe visual DSL

Add a visual DSL for seesaw handler `describe` functions. Gate handlers return `Vec<Block>` — a shared vocabulary of visual primitives that the admin flow UI renders inline on each node.

## Context

- Brainstorm: `docs/brainstorms/2026-03-06-describe-visual-dsl-brainstorm.md`
- Seesaw 0.23.1 added `#[handle(describe = describe_fn)]` — persists serialized output per `(correlation_id, handler_id)` via `Store::set_handler_descriptions`
- `PostgresStore` currently returns default no-op for `set_handler_descriptions` / `get_handler_descriptions` — needs real implementation

## Acceptance Criteria

- [x] `Block` enum with 6 primitives in `rootsignal-common/src/describe.rs`
- [x] `PostgresStore` implements `set_handler_descriptions` + `get_handler_descriptions`
- [x] SQL migration for `seesaw_handler_descriptions` table
- [x] Describe functions wired to gate handlers (scrape, enrichment, synthesis)
- [x] GraphQL query exposes describe output per run
- [x] Round-trip serde tests for all Block variants

## Implementation

### 1. Add `rootsignal-common/src/describe.rs`

```rust
// rootsignal-common/src/describe.rs

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Label { text: String },
    Counter { label: String, value: u32, total: u32 },
    Progress { label: String, fraction: f32 },
    Checklist { label: String, items: Vec<ChecklistItem> },
    KeyValue { key: String, value: String },
    Status { label: String, state: State },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Waiting,
    Running,
    Done,
    Error,
}
```

Wire into `rootsignal-common/src/lib.rs`:
```rust
pub mod describe;
pub use describe::*;
```

### 2. SQL migration: `seesaw_handler_descriptions`

```sql
-- migrations/NNNN_seesaw_handler_descriptions.sql
CREATE TABLE IF NOT EXISTS seesaw_handler_descriptions (
    correlation_id UUID NOT NULL,
    handler_id     TEXT NOT NULL,
    description    JSONB NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (correlation_id, handler_id)
);
```

### 3. `PostgresStore` — implement Store trait methods

File: `modules/rootsignal-scout/src/core/postgres_store.rs`

```rust
async fn set_handler_descriptions(
    &self,
    correlation_id: Uuid,
    descriptions: HashMap<String, serde_json::Value>,
) -> Result<()> {
    for (handler_id, data) in descriptions {
        sqlx::query(
            "INSERT INTO seesaw_handler_descriptions \
             (correlation_id, handler_id, description, updated_at) \
             VALUES ($1, $2, $3, now()) \
             ON CONFLICT (correlation_id, handler_id) \
             DO UPDATE SET description = EXCLUDED.description, updated_at = now()"
        )
        .bind(correlation_id)
        .bind(&handler_id)
        .bind(&data)
        .execute(&self.pool)
        .await?;
    }
    Ok(())
}

async fn get_handler_descriptions(
    &self,
    correlation_id: Uuid,
) -> Result<HashMap<String, serde_json::Value>> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT handler_id, description FROM seesaw_handler_descriptions \
         WHERE correlation_id = $1"
    )
    .bind(correlation_id)
    .fetch_all(&self.pool)
    .await?;
    Ok(rows.into_iter().collect())
}
```

### 4. Describe functions on gate handlers

Each domain's `mod.rs` gets a describe function wired via `describe = describe_fn` attribute.

#### `scrape/mod.rs` — scrape gate handlers

Handlers with `is_sources_prepared` filter (fetch_web, fetch_social) are simple event-match filters, not stateful gates — no describe needed.

Handlers to add describe to:
- Any handler gated on `completed_scrape_roles` (check if any exist after recent refactors)

#### `enrichment/mod.rs` — enrichment gates

```rust
fn describe_enrichment_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let all = all_enrichment_roles();
    let done = &state.completed_enrichment_roles;
    vec![
        Block::Checklist {
            label: "Enrichment roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
    ]
}
```

Add `describe = describe_enrichment_gate` to enrichment handlers that use state-gated filters (e.g., `response_done_actor_extraction_pending`, `response_done_diversity_pending`, etc.).

#### `synthesis/mod.rs` — synthesis gate

```rust
fn describe_synthesis_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let all = all_synthesis_roles();
    let done = &state.completed_synthesis_roles;
    vec![
        Block::Checklist {
            label: "Synthesis roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
    ]
}
```

Add `describe = describe_synthesis_gate` to `synthesis:infer_severity` and any other handlers gated on `all_synthesis_done`.

#### `lifecycle/mod.rs` — finalize gates

The finalize handlers (`finalize_scrape_run`, `finalize_full_run`) also gate on synthesis/scrape completion — wire describe functions showing what they're waiting for.

### 5. GraphQL query

File: `modules/rootsignal-api/src/graphql/schema.rs`

```rust
#[derive(SimpleObject)]
struct HandlerDescription {
    handler_id: String,
    blocks: serde_json::Value,
}

// In QueryRoot impl:
#[graphql(guard = "AdminGuard")]
async fn admin_handler_descriptions(
    &self,
    ctx: &async_graphql::Context<'_>,
    run_id: String,
) -> Result<Vec<HandlerDescription>> {
    let pool = ctx.data::<PgPool>()?;
    let run_uuid = Uuid::parse_str(&run_id)?;
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT handler_id, description FROM seesaw_handler_descriptions \
         WHERE correlation_id = $1"
    )
    .bind(run_uuid)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(handler_id, blocks)| HandlerDescription { handler_id, blocks }).collect())
}
```

### 6. Tests

#### `rootsignal-common` — serde round-trip

```rust
// rootsignal-common/src/describe.rs #[cfg(test)]
#[test]
fn block_variants_round_trip_through_json() {
    let blocks = vec![
        Block::Label { text: "hello".into() },
        Block::Counter { label: "x".into(), value: 3, total: 5 },
        Block::Progress { label: "p".into(), fraction: 0.5 },
        Block::Checklist { label: "c".into(), items: vec![
            ChecklistItem { text: "a".into(), done: true },
            ChecklistItem { text: "b".into(), done: false },
        ]},
        Block::KeyValue { key: "k".into(), value: "v".into() },
        Block::Status { label: "s".into(), state: State::Running },
    ];
    let json = serde_json::to_string(&blocks).unwrap();
    let parsed: Vec<Block> = serde_json::from_str(&json).unwrap();
    assert_eq!(blocks.len(), parsed.len());
}
```

#### `rootsignal-scout` — describe function unit tests

Test that describe functions return expected blocks given a PipelineState with known completed roles.

### 7. Admin app — render describe blocks on handler nodes

The flow graph lives in `CausalFlowPane.tsx`. Handler nodes are currently `default` React Flow nodes with inline styling. To render describe blocks, we need:

#### a. GraphQL query

File: `modules/admin-app/src/graphql/queries.ts`

```ts
export const ADMIN_HANDLER_DESCRIPTIONS = gql`
  query AdminHandlerDescriptions($runId: String!) {
    adminHandlerDescriptions(runId: $runId) {
      handlerId
      blocks
    }
  }
`;
```

#### b. Custom handler node component

File: `modules/admin-app/src/pages/events/components/HandlerNode.tsx` (new)

A custom React Flow node type that renders:
- Handler ID label (existing italic text)
- Below it: describe blocks from the fetched data

Block renderers (all compact, fitting inside a ~200px wide node):
- **Label**: small gray text
- **Counter**: `"3 / 5"` with label
- **Progress**: thin bar (CSS `background: linear-gradient(...)`)
- **Checklist**: small checkmarks/crosses with item text
- **KeyValue**: `key: value` in monospace
- **Status**: colored dot + label (`waiting`=yellow, `running`=blue, `done`=green, `error`=red)

#### c. Wire into CausalFlowPane

File: `modules/admin-app/src/pages/events/panes/CausalFlowPane.tsx`

1. Fetch `ADMIN_HANDLER_DESCRIPTIONS` when `flowRunId` is set
2. Pass describe data into handler node's `data` prop
3. Register `HandlerNode` as a custom `nodeTypes` on `<ReactFlow>`
4. Use `type: "handler"` instead of `type: "default"` for handler nodes
5. Increase `HANDLER_HEIGHT` dynamically based on number of blocks (dagre needs accurate heights for layout)

#### d. TypeScript types

File: `modules/admin-app/src/types/describe.ts` (new)

```ts
type Block =
  | { type: "label"; text: string }
  | { type: "counter"; label: string; value: number; total: number }
  | { type: "progress"; label: string; fraction: number }
  | { type: "checklist"; label: string; items: { text: string; done: boolean }[] }
  | { type: "key_value"; key: string; value: string }
  | { type: "status"; label: string; state: "waiting" | "running" | "done" | "error" };
```

These mirror the Rust `Block` enum's serde output exactly (internally tagged with `"type"`, snake_case).

## Files Changed

| File | Change |
|------|--------|
| `rootsignal-common/src/describe.rs` | New — Block enum, ChecklistItem, State |
| `rootsignal-common/src/lib.rs` | Add `pub mod describe; pub use describe::*;` |
| `rootsignal-scout/src/core/postgres_store.rs` | Implement `set_handler_descriptions` + `get_handler_descriptions` |
| `migrations/NNNN_seesaw_handler_descriptions.sql` | New table |
| `rootsignal-scout/src/domains/enrichment/mod.rs` | Add describe fn to enrichment gates |
| `rootsignal-scout/src/domains/synthesis/mod.rs` | Add describe fn to synthesis gates |
| `rootsignal-scout/src/domains/lifecycle/mod.rs` | Add describe fn to finalize gates |
| `rootsignal-api/src/graphql/schema.rs` | Add `admin_handler_descriptions` query |
| `admin-app/src/graphql/queries.ts` | Add `ADMIN_HANDLER_DESCRIPTIONS` query |
| `admin-app/src/types/describe.ts` | New — TypeScript Block discriminated union |
| `admin-app/src/pages/events/components/HandlerNode.tsx` | New — custom React Flow node for handlers with block rendering |
| `admin-app/src/pages/events/panes/CausalFlowPane.tsx` | Fetch descriptions, register custom node type, dynamic handler heights |

## Verification

```bash
# Backend
cargo check -p rootsignal-common
cargo test -p rootsignal-common
cargo check -p rootsignal-scout
cargo test -p rootsignal-scout
cargo check -p rootsignal-api

# Frontend
cd modules/admin-app && npm run build
```
