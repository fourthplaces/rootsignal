# Testing

## Philosophy

Tests follow three rules from `CLAUDE.md`:

1. **MOCK → FUNCTION → OUTPUT**: Set up mocks, call the real function under test, assert the output. Never manually call internal functions step-by-step — the organ does the wiring; tests only check what came out.

2. **Behavior names**: Test names describe what the system does, not how. A reader should understand system behavior from the name alone. Good: `blank_author_name_does_not_create_actor`. Bad: `whitespace_only_author_actor_not_created`.

3. **Testability drives architecture**: If code cannot be tested with MOCK → FUNCTION → OUTPUT (e.g., it takes concrete types instead of traits), refactor the code — the testing constraint drives the architecture.

## Four Test Levels

### 1. Activity Tests (Unit)

Test individual activity functions in isolation with mocked dependencies.

```
Location: domains/{domain}/activities/*_tests.rs
Pattern:  Mock deps → call activity function → assert returned Events
Example:  dedup_tests.rs, creation_tests.rs
```

Activity functions take trait objects (`&dyn SignalReader`, `&dyn TextEmbedder`) and return `Events`. Mocks control all external behavior.

### 2. Boundary Tests

Verify one handler-to-handler handoff at a time.

```
Location: domains/{domain}/boundary_tests.rs
Pattern:  Build engine with mocks → emit trigger event → settle → assert emitted events
Example:  scrape/boundary_tests.rs
```

Uses `ScoutEngineDeps.captured_events` to capture all events dispatched during settle. Asserts that the right events were emitted in the right order.

### 3. Chain Tests

Test multi-handler causal chains end-to-end with mocked I/O.

```
Location: domains/{domain}/chain_tests.rs
Pattern:  Build engine → emit entry event → settle → assert final aggregate state + events
Example:  scrape/chain_tests.rs, signals/engine_tests.rs
```

These test the complete pipeline sub-chains (e.g., scrape → extract → dedup → create → wire) with all handlers registered.

### 4. SimWeb Scenarios (Integration)

Full pipeline with LLM-generated content via the `simweb` crate.

```
Location: (separate crate)
Pattern:  SimWeb generates fake web pages → full engine run → assert graph state
```

## Mock Implementations

All mocks are hand-written (no mock frameworks). They live in `src/testing.rs`:

| Mock | Trait | Behavior |
|------|-------|----------|
| `MockFetcher` | `ContentFetcher` | HashMap-based URL → content mapping |
| `MockSignalReader` | `SignalReader` | Configurable return values for graph queries |
| `FixedEmbedder` | `TextEmbedder` | Deterministic vectors from content hash — reproducible similarity scores |
| `MockExtractor` | `SignalExtractor` | Returns preconfigured extraction results |

## Test Event Capture

The engine supports test-only event capture via `ScoutEngineDeps.captured_events`:

```rust
let sink = Arc::new(Mutex::new(Vec::new()));
let deps = ScoutEngineDeps::new(store, embedder, "test-run");
deps.captured_events = Some(sink.clone());

let engine = build_engine(deps);
engine.emit(event).await;
engine.settle().await?;

let events = sink.lock().unwrap();
// Assert on captured events via downcast_ref
```

When `captured_events` is `Some`, the engine registers a `capture_handler` (priority 0) that pushes every `AnyEvent` into the shared `Vec`.

## Aggregate Assertions

After settle, test code reads final state from the engine's aggregate:

```rust
let state: Arc<PipelineState> = engine.singleton::<PipelineState>();
assert_eq!(state.stats.signals_stored, 3);
assert_eq!(state.stats.signals_deduplicated, 1);
```

## Deterministic Embedding Tests

`FixedEmbedder` generates deterministic vectors from content hashes, making dedup threshold tests reproducible. Two pieces of content will always produce the same similarity score across runs, enabling precise threshold assertions.

## Key Testing Patterns

- **No I/O in tests**: All external calls (Neo4j, Postgres, Voyage AI, Claude, web fetching) are behind traits and mocked.
- **Event-driven assertions**: Tests assert on emitted events, not internal state transitions.
- **Aggregate state for final assertions**: After settle, read `engine.singleton()` for cumulative state checks.
- **Optional deps for isolation**: `graph_projector: None`, `event_store: None`, etc. allow handler tests without infrastructure.
