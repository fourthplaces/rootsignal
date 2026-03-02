# Event-Sourcing Exceptions

Components that bypass the event-sourced causal chain by writing directly to Neo4j. Each is documented with the rationale for keeping it as a direct write.

## 1. Task phase_status (workflows/mod.rs)

`write_task_phase_status` and `journaled_write_task_phase_status` write directly to Neo4j. These are Restate workflow observability bookkeeping — they happen before/after engines exist, inside replay-sensitive `ctx.run()` closures, and serve the admin UI. `transition_task_phase_status` is a distributed CAS lock. None are domain facts.

## 2. Embedding enrichment (rootsignal-graph/src/embedding_enrichment.rs)

`SET n.embedding` — derived computed properties. Event-sourcing would bloat the store with large float vectors on every enrichment pass.

## 3. Diversity/actor stats (rootsignal-graph/src/enrich.rs)

`source_diversity`, `channel_diversity`, `external_ratio`, `signal_count`. Materialized aggregates recomputed from graph state. Not domain decisions.

## 4. actor_extractor reads (enrichment/activities/actor_extractor.rs)

Direct `GraphClient` reads for signal data. Future: add `signals_without_actors()` to the `SignalReader` trait.

## 5. ScrapePhase struct (scrape/activities/scrape_phase.rs)

Carries `GraphClient` for content-hash lookups during scraping. Future: add hash lookup to `ContentFetcher` or `SignalReader`.

## 6. Bootstrap join (workflows/bootstrap.rs)

`graph.get_source_nodes()` at pipeline start. Acceptable bootstrapping read — no writes.
