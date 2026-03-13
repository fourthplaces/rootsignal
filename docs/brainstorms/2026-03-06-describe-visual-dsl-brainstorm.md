---
date: 2026-03-06
topic: describe-visual-dsl
---

# Handler Describe Visual DSL

## What We're Building

A lightweight visual DSL for seesaw handler `describe` functions. Each handler with a stateful gate can return a `Vec<Block>` that the admin flow UI renders inline on each node. The DSL defines a shared vocabulary of visual primitives — the handler side composes them, the frontend renders them. Neither side knows about the other's internals.

This leverages seesaw 0.23's `#[handle(describe = describe_fn)]` attribute, which persists the serialized output per `(correlation_id, handler_id)` so the API can serve it to the frontend without touching the engine.

## Why This Approach

We considered two approaches:
- **A: Handler-scoped aggregates** — a new seesaw concept where each handler gets its own aggregate. Powerful but heavyweight; doesn't earn its weight for our ~2 gate handlers.
- **B: Compose with existing aggregates via `describe()`** — handlers already have access to `PipelineState` via `ctx.singleton()`. A `describe` function reads that state and returns structured output. Earns its weight because flow UI nodes need *something* to show.

The DSL is the "something." Without it, every describe function returns ad-hoc JSON and the frontend needs per-handler rendering logic. With it, the frontend has one renderer and handlers compose from shared blocks.

## Primitives

Six building blocks. Each handler's describe returns `Vec<Block>`.

### Label
Static text. Title, explanation, status message.
```rust
Block::Label { text: "Waiting for scrape phase" }
```

### Counter
Numeric value against a known total.
```rust
Block::Counter { label: "Sources scraped", value: 7, total: 12 }
```

### Progress
Fractional completion as a float (0.0–1.0). Frontend renders as a bar.
```rust
Block::Progress { label: "Scrape progress", fraction: 0.58 }
```

### Checklist
Items with done/not-done state. The gate's natural representation.
```rust
Block::Checklist {
    label: "Synthesis roles",
    items: vec![
        ChecklistItem { text: "GatheringFinder", done: true },
        ChecklistItem { text: "ResponseFinder", done: true },
        ChecklistItem { text: "ConcernLinker", done: false },
    ],
}
```

### KeyValue
Structured pair. Frontend can render the value distinctly (bold, monospace, colored).
```rust
Block::KeyValue { key: "Region", value: "Portland, OR" }
```

### Status
Semantic state badge. Frontend maps to colors/icons.
```rust
Block::Status { label: "Scrape phase", state: State::Running }
// State: Waiting, Running, Done, Error
```

## Where It Lives

`rootsignal-common` — shared between scout (produces) and API (serves via GraphQL). The types are domain-agnostic; they're visual primitives, not scout concepts.

```
rootsignal-common/src/describe.rs
```

## Example: Scrape Gate Describe

```rust
fn describe_scrape_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let all = all_scrape_roles();
    let done = &state.completed_scrape_roles;

    vec![
        Block::Checklist {
            label: "Scrape roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
        Block::Counter {
            label: "Progress".into(),
            value: done.len() as u32,
            total: all.len() as u32,
        },
    ]
}
```

## Decisions

- **State enum**: Fixed — `Waiting | Running | Done | Error`. Add variants if we need them later.
- **Block keying**: Index in vec. Frontend concern, not backend's job.
- **API serving**: seesaw persists describe output to the Store per `(correlation_id, handler_id)`. We query it from SQL directly — no special engine plumbing needed. The API can serve it however makes sense (GraphQL field, REST, etc.).

## Next Steps

-> `/workflows:plan` for implementation
