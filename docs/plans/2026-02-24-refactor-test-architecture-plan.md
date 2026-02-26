---
title: "refactor: Test architecture — enforce MOCK → FUNCTION → OUTPUT"
type: refactor
date: 2026-02-24
---

# refactor: Test architecture — enforce MOCK → FUNCTION → OUTPUT

## Overview

Multiple test files bypass the code they're supposed to test. `litmus_test.rs` creates graph data via raw Cypher instead of `GraphWriter`. `extraction_test.rs` reimplements 280 lines of conversion logic instead of calling the real converter. This refactoring fixes the test architecture so every test follows MOCK → FUNCTION → OUTPUT with no code taken out of context.

## Problem Statement

### litmus_test.rs (rootsignal-graph)

~60 tests use hand-rolled Cypher helpers (`create_signal`, `create_gathering_with_date`, `create_actor_and_link`) to set up graph data. These helpers duplicate what `GraphWriter::create_node()`, `upsert_actor()`, and `link_actor_to_signal()` do — but with subtle differences (different property sets, different Cypher patterns). The tests prove Neo4j works, not that our write layer works.

### extraction_test.rs (rootsignal-scout)

Snapshot replay calls a test-local `convert_response()` (150 lines) that reimplements the `ExtractionResponse → ExtractionResult` conversion from `extract_impl`. A companion `extraction_result_to_response()` (130 lines) does the reverse for recording. If someone changes the real converter, the test copy diverges silently.

### Root cause

The conversion logic inside `extract_impl` is private and welded to the LLM call. There's no way to call it without an LLM round-trip, so tests either reimplement it or skip it.

## System Boundaries

The data flow has four natural test boundaries:

```
Content → LLM → ExtractionResponse → [conversion] → ExtractionResult → GraphWriter → Neo4j → PublicGraphReader → API
                                       ^^^^^^^^^^^                        ^^^^^^^^^^^
                                       Layer 1: pure logic                Layer 2: persistence
```

| Layer | Input | Function under test | Output | I/O |
|---|---|---|---|---|
| 1. Conversion | ExtractionResponse JSON | Extractor::convert_signals() | ExtractionResult | None |
| 2. Write | Node structs | GraphWriter::create_node, create_evidence, etc. | Graph state | Neo4j |
| 3. Read | Graph state | PublicGraphReader methods | Typed results | Neo4j |
| 4. Extraction quality | Fixture content + LLM snapshot | convert_signals() | Domain correctness | None |

Layers 1, 2, and 4 are in scope. Layer 3 (reader coverage) is a separate future effort.

## Phase 1: Extract convert_signals (rootsignal-scout)

**Goal:** Make the ExtractionResponse → ExtractionResult conversion callable without an LLM.

**Change:** Extract the conversion logic from `extract_impl` (extractor.rs:256-522) into a public associated function:

```rust
impl Extractor {
    /// Convert raw LLM extraction output into typed domain nodes.
    ///
    /// Pure transformation — no I/O, no LLM. Handles:
    /// - Junk signal filtering ("unable to extract", "page not found")
    /// - Non-firsthand signal rejection
    /// - Sensitivity/severity/urgency string → enum mapping
    /// - Date parsing (RFC3339 → DateTime<Utc>)
    /// - Geo precision mapping
    /// - RRULE validation
    /// - Tag slugification
    /// - source_url fallback (signal-level → page-level)
    pub fn convert_signals(
        response: ExtractionResponse,
        source_url: &str,
    ) -> ExtractionResult {
        // ... moved from extract_impl
    }
}
```

`extract_impl` becomes:

```rust
async fn extract_impl(&self, content: &str, source_url: &str) -> Result<ExtractionResult> {
    let content = /* truncation logic */;
    let user_prompt = /* build prompt */;
    let response: ExtractionResponse = self.claude.extract(&self.system_prompt, &user_prompt).await?;
    Ok(Self::convert_signals(response, source_url))
}
```

**Why associated function:** The conversion is stateless — `self` is never touched after the LLM call returns. An associated function (not a method) makes this explicit. It lives on `Extractor` because the conversion embodies extraction policy (filtering rules, enum defaults, fallback logic).

**Verification:** `ExtractionResponse` and `ExtractionResult` are already public. This completes the existing API surface.

### Acceptance criteria

- [ ] `extract_impl` calls `Self::convert_signals()` instead of inline conversion
- [ ] All existing tests pass without modification (behavior-preserving refactor)
- [ ] `convert_signals` is `pub` and takes no `self` parameter

## Phase 2: Write conversion_test.rs (rootsignal-scout)

**Goal:** Test conversion edge cases that the snapshot tests don't cover.

**File:** `modules/rootsignal-scout/tests/conversion_test.rs`

Each test follows: hand-craft ExtractionResponse JSON → `Extractor::convert_signals()` → assert ExtractionResult.

### Test scenarios

| Test name | Mock (ExtractionResponse) | Assertion |
|---|---|---|
| `junk_title_filtered` | signal with title "Unable to extract content" | nodes is empty, rejected has reason "junk_extraction" |
| `non_firsthand_signal_rejected` | signal with `is_firsthand: false` | nodes is empty, rejected has reason "not_firsthand" |
| `firsthand_null_signal_kept` | signal with `is_firsthand` absent | node is present (null means keep) |
| `gathering_date_parsed_from_rfc3339` | gathering with starts_at "2026-04-12T18:00:00Z" | GatheringNode.starts_at is April 12 2026 |
| `invalid_date_becomes_none` | gathering with starts_at "not-a-date" | GatheringNode.starts_at is None |
| `missing_date_becomes_none` | gathering with starts_at absent | GatheringNode.starts_at is None |
| `sensitivity_maps_to_enum` | signals with "sensitive", "elevated", "general", "unknown" | Sensitive, Elevated, General, General |
| `severity_maps_to_enum` | tensions with "critical", "high", "medium", "low", "unknown" | Critical, High, Medium, Low, Medium |
| `urgency_maps_to_enum` | needs with "critical", "high", "medium", "low", "unknown" | Critical, High, Medium, Low, Medium |
| `geo_precision_maps_correctly` | signals with "exact", "neighborhood", "other", absent | Exact, Neighborhood, Approximate, None |
| `signal_source_url_overrides_page_url` | signal with source_url set | node.meta.source_url is signal-level URL |
| `missing_signal_source_url_falls_back_to_page` | signal with source_url absent | node.meta.source_url is page-level URL |
| `unknown_signal_type_skipped` | signal with signal_type "unknown_thing" | nodes is empty (no error) |
| `tags_slugified` | signal with tags ["Community Garden", "FOOD pantry"] | slugified tags on result |
| `valid_rrule_produces_schedule` | gathering with valid RRULE | schedules vec is non-empty |
| `invalid_rrule_falls_back_to_schedule_text` | gathering with invalid RRULE + schedule_text | schedule has None rrule, Some schedule_text |
| `resource_tags_collected` | signal with resources | resource_tags paired with node UUID |
| `implied_queries_aggregated` | multiple signals with implied_queries | all queries in result.implied_queries |
| `aid_defaults_is_ongoing_true` | aid signal with is_ongoing absent | AidNode.is_ongoing is true |
| `gathering_defaults_is_recurring_false` | gathering with is_recurring absent | GatheringNode.is_recurring is false |

### Acceptance criteria

- [ ] All tests pass
- [ ] Every test is MOCK → `Extractor::convert_signals()` → assertion
- [ ] No I/O (no Neo4j, no LLM, no filesystem)

## Phase 3: Clean up extraction_test.rs (rootsignal-scout)

**Goal:** Delete duplicated conversion logic; use real converter.

### Changes

1. **Delete** `convert_response()` (~150 lines, current lines 98-246)
2. **Delete** `extraction_result_to_response()` (~130 lines, current lines 251-382)
3. **Update** `load_or_record()` replay path:

```rust
// Before (calls test-local reimplementation):
let result = convert_response(&response, url);

// After (calls real converter):
let result = Extractor::convert_signals(response.clone(), url);
```

4. **Update** `load_or_record()` record path to snapshot the `ExtractionResponse` directly from the LLM (instead of round-tripping through `extraction_result_to_response`).

### Snapshot compatibility

Existing snapshots were saved via `extraction_result_to_response()` — a lossy reverse mapping from `ExtractionResult` back to `ExtractionResponse`. These may not be valid `ExtractionResponse` JSON if the reverse mapping dropped or mangled fields. **Action:** verify each snapshot deserializes through `Extractor::convert_signals()` before deleting the old code. Re-record any that fail.

### Acceptance criteria

- [ ] `convert_response` and `extraction_result_to_response` deleted
- [ ] All extraction tests pass using `Extractor::convert_signals()`
- [ ] Snapshot replay tests the real conversion pipeline
- [ ] Zero lines of reimplemented conversion logic in tests

## Phase 4: Rewrite litmus_test.rs setup (rootsignal-graph)

**Goal:** All litmus tests create data through `GraphWriter`, not raw Cypher.

### Changes

1. **Delete** raw Cypher helpers:
   - `create_signal()` (~50 lines)
   - `create_signal_at()` (~40 lines)
   - `create_gathering_with_date()` (~35 lines)
   - `create_actor_and_link()` (~40 lines)
   - `dummy_embedding()` string-literal version (~5 lines)

2. **Replace** with typed helper functions that call `GraphWriter`:

```rust
/// Build a minimal Gathering node for testing.
fn gathering(title: &str, lat: f64, lng: f64) -> Node {
    Node::Gathering(GatheringNode {
        meta: test_meta(title, lat, lng),
        starts_at: None,
        ends_at: None,
        action_url: String::new(),
        organizer: None,
        is_recurring: false,
    })
}

/// Build a Gathering with a specific starts_at.
fn gathering_at(title: &str, starts_at: DateTime<Utc>) -> Node {
    Node::Gathering(GatheringNode {
        meta: test_meta(title, 44.9778, -93.2650),
        starts_at: Some(starts_at),
        ..Default-ish
    })
}

// Similar: aid(), need(), tension(), notice()
```

Setup in each test becomes:

```rust
// Before:
create_signal(&client, "Aid", id, "Free food", "https://food.org").await;

// After:
let node = aid("Free food", 44.9778, -93.2650);
writer.create_node(&node, &dummy_embedding(), "test", "test-run").await.unwrap();
```

3. **Actor setup** switches from raw Cypher to:

```rust
writer.upsert_actor(&actor_node).await.unwrap();
writer.link_actor_to_signal(actor_id, signal_id).await.unwrap();
```

4. **Evidence tests** (9-11) already use `GraphWriter` for evidence — switch their signal setup from raw Cypher to `GraphWriter::create_node()` for consistency.

5. **Assertion queries stay as raw Cypher.** These are verification — they check what's in the graph. Raw Cypher is the most direct way to verify and doesn't need to go through our code.

### Risk: property mismatches

`GraphWriter::create_node()` may set different properties than the raw Cypher helpers did. This is **intentional** — if the tests break, it means the raw Cypher was testing a graph state that `GraphWriter` never actually produces. Those tests were providing false confidence. Failures here surface real gaps.

### Acceptance criteria

- [ ] Zero raw Cypher `CREATE` statements in test setup (only in assertions)
- [ ] All litmus tests use `GraphWriter` for data creation
- [ ] All tests pass (or failures are documented as real bugs in GraphWriter)
- [ ] Helper functions build typed `Node` structs, not Cypher strings

## Out of Scope

- **PublicGraphReader test coverage** — The litmus tests that verify query behavior (keyword search, bbox filtering, sorting) will still assert via raw Cypher. Moving them to use `PublicGraphReader` is a separate effort that should happen once the write side is clean.
- **graph_write_test.rs changes** — This file already follows the correct pattern (uses `GraphWriter` for creation). No changes needed unless litmus rewrites create duplication.
- **bbox_scoping_test.rs** — These are intentionally `#[ignore]`d smoke tests against a live DB. Keep as-is.

## Execution Order

Phases must be done in order — each builds on the previous:

1. **Phase 1** unlocks phases 2 and 3 (they need `convert_signals` to exist)
2. **Phase 2** can be done in parallel with Phase 3 after Phase 1 lands
3. **Phase 4** is independent of phases 1-3 (different crate) but benefits from the pattern established in Phase 2

## Net Effect

| Metric | Before | After |
|---|---|---|
| Duplicated conversion logic | 280 lines in extraction_test.rs | 0 |
| Raw Cypher setup helpers | ~170 lines in litmus_test.rs | 0 |
| Conversion logic test coverage | 0 (only deserialization tested) | ~20 edge case tests |
| Tests that actually call GraphWriter | graph_write_test.rs only | graph_write_test.rs + litmus_test.rs |
| Untested behaviors | junk filtering, firsthand rejection, date parsing, RRULE validation, tag slugification, enum defaults | All covered |
