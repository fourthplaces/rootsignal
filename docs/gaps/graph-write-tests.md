# Gap: Graph Write Tests

## Deleted

`modules/rootsignal-scout/tests/graph_write_test.rs` — removed 2026-02-26.

## What was covered

- `signals_get_evidence_trail` — every signal has SOURCED_FROM evidence
- `dedup_same_signal_yields_one_node` — same signal ID written twice
- `multiple_signal_types_store_correctly` — Gathering, Aid, Tension labels
- `duplicate_evidence_source_url_merges` — evidence MERGE on source_url
- `long_text_fields_stored_correctly` — 1000-char title, 5000-char summary
- `evidence_for_missing_signal_is_noop` — phantom signal ID
- `special_characters_stored_safely` — Unicode, quotes, Cypher injection
- `schedule_node_created_and_linked_to_gathering` — ScheduleNode + HAS_SCHEDULE
- `schedule_with_rrule_and_exdates_stored_correctly` — rdates/exdates arrays
- `schedule_text_only_fallback_works` — text-only Schedule without rrule

## Where coverage now lives

- Signal creation + evidence trails: tested via Pipeline pattern in `litmus_test.rs` and `pipeline_test.rs`
- Deduplication: tested via `ScrapePhase` corroboration in `chain_tests.rs`
- Actor linking: tested in `chain_tests.rs` social scrape tests

## What's missing

- Direct `GraphWriter` unit tests (methods were removed/refactored)
- Schedule node creation and linking via graph layer
- Long text / special character round-trip at the graph layer
- Signal lint tests (`signal_lint_test.rs` deleted — `signal_lint.rs` and `lint_tools.rs` call
  SignalStore methods that don't exist on the trait yet: `staged_signals_in_region`,
  `promote_ready_situations`, `promote_ready_stories`, `set_review_status`,
  `update_signal_fields`, `set_signal_corrected`)
