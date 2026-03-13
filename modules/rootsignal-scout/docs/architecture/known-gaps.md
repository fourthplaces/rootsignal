# Known Gaps

Architectural gaps identified from audits and brainstorms. These are known trade-offs, not bugs.

## Event Sourcing

### No Replay Tooling
The event store supports replay (append-only, monotonic sequences), but no CLI or command exists to replay events and rebuild the graph. Seesaw's `AggregateLoader` with `replay_events()` provides the mechanism — needs a harness.

### No Event Upcasters
The `schema_v` column exists in the event store but no upcasting logic is registered. When event schemas evolve, old events need transformation before replay. Seesaw provides the `EventUpcast` trait — needs implementation for each event type migration.

### No Snapshots
Full replay from event zero is fine at current scale. At higher event volumes, aggregate snapshots will be needed to avoid replaying the entire history. Seesaw's `AggregatorRegistry` supports `set_state()` and `get_version()` for snapshot hydration.

### No Mid-Chain Failure Recovery
If a handler fails partway through a causal chain, the events already persisted are committed but the chain is incomplete. No compensation, saga, or retry mechanism exists. The current approach is to re-run the entire workflow (idempotent by design).

### Replay-Aware Side Effects
Neo4j `MERGE` is idempotent, making current projections replay-safe. If future handlers have non-idempotent side effects (emails, webhooks, external API calls), they will need replay detection via `ctx.run()` (seesaw's side-effect journaling).

## Abstractions

### Shared Types Need Extraction
Several types leak across domain boundaries and should live in `core/`:
- `CollectedLink` (defined in `enrichment`, used by `scrape` and `discovery`)
- `ScrapeOutput` / `ExpansionOutput` (activity output types referenced by aggregate apply methods)

### Missing Trait Abstractions
- `GraphClient` is a concrete type, not a trait — hard to mock in tests
- `Archive` is a concrete type used as `ContentFetcher` — the trait boundary exists but could be cleaner
- LLM model strings are hardcoded in 13+ locations instead of being configurable

### Activities Receive Full Deps
Many activity functions take `&ScoutEngineDeps` wholesale instead of the specific deps they need. This makes dependency boundaries unclear and testing harder than necessary.

## Data Quality

### Actor Fuzzy Matching
Actor dedup uses exact string matching only. "Simpson Housing" and "Simpson Housing Services" create two separate actor nodes. Fuzzy matching (edit distance, LLM-assisted) is deferred.

### URL Normalization
Three overlapping URL normalization systems exist:
- `canonical_value` (graph-level canonical keys)
- `sanitize_url()` (strip tracking params)
- `strip_tracking_params()` (separate implementation)

These should be unified into a single normalization pipeline.

### Source Location Stamping
`promote_links` stamps every promoted source with the discovering region's center coordinates, regardless of the source's actual geographic coverage. This creates inaccurate source locations.

## Dead Code

Identified but not yet cleaned up:
- `PipelinePhase::SocialScrape` and `SocialDiscovery` — unused phase variants
- `platform_prefix` — dead code path

## Observability

### Run Log Uses JSONB
`run_log.rs` stores `EventKind` as a single `data JSONB` column with a denormalized `event_type` string for filtering. This replaced the previous 39-column approach. GraphQL API queries extract fields from JSONB at read time. Historical events before the JSONB migration will not have `data` populated.

## Future Work

### Phase 7: Delete rootsignal-engine
The old engine crate is superseded by seesaw. Can be deleted once all references are removed.

### Phase 8: Collapse Orchestrator
The orchestrator files (`scrape_pipeline.rs`, `scrape_phase.rs`, ~4,600 lines) predate the event engine. Their logic is now in domain handlers. The files can be removed once the migration is fully validated.

### Restate Durable Execution
Currently using Restate for workflow orchestration but not leveraging durable execution (journaling, replay). Parked until 5+ cities, long-running failures, or human-in-the-loop workflows justify the complexity.
