# Overview

## What Scout Does

A single scout run discovers, extracts, deduplicates, and graphs signals for a geographic region. The pipeline runs as a seesaw event engine — a single entry event (`EngineStarted`) triggers a causal chain of handlers that drive the entire run to completion.

## Data Model

Scout operates on six signal node types sharing a common `NodeMeta`:

| Type | Description | Expiry |
|------|-------------|--------|
| **Gathering** | Time-bound events — protests, cleanups, workshops, meetings | 30 days past `ends_at` |
| **Resource** | Available resources — food shelves, free clinics, tool libraries | 60 days without re-confirmation |
| **HelpRequest** | Community requests — volunteer calls, donation drives | 60 days |
| **Announcement** | Official advisories — policy changes, shelter openings | 90 days |
| **Concern** | Systemic conflicts — housing crises, environmental harm | Never (persistent) |
| **Condition** | Environmental/infrastructure state — road closures, air quality | 30 days |

Supporting nodes: `Citation` (source evidence), `Actor` (organizations/people), `Source` (data feeds), `Resource` (links/documents), `Tag` (categorization).

## Module Map

```
src/
  core/
    engine.rs          ScoutEngineDeps, build_engine(), build_full_engine()
    aggregate.rs       PipelineState singleton aggregate + seesaw aggregators
    projection.rs      Infrastructure handlers: persist (priority 0), neo4j_projection (priority 2)
    pipeline_events.rs PipelineEvent enum (aggregate-mutation bookkeeping)
    events.rs          PipelinePhase + FreshnessBucket shared enums
    stats.rs           ScoutStats — cumulative run metrics
    extractor.rs       SignalExtractor trait + LLM implementation
    embedding_cache.rs In-memory cross-batch dedup cache (cosine similarity)

  domains/
    lifecycle/         Reap, schedule, finalize — pipeline orchestration
    scrape/            Web + social content fetching and signal extraction
    signals/           Dedup, creation, edge wiring — signal processing sub-chain
    discovery/         Bootstrap, link promotion, mid-run source discovery
    enrichment/        Actor extraction, location triangulation, source metrics
    expansion/         Signal expansion from implied queries
    synthesis/         Similarity edges, response mapping, agentic finders (6 parallel roles)
      util.rs          Shared finder utilities: region bounds, node builders, tension matching
    situation_weaving/ Leiden clustering, narrative generation, curiosity triggers
    supervisor/        Issue detection, duplicate merging, cause heat, beacons
    scheduling/        Budget tracking, source scheduling (utility, no handlers)

  infra/
    embedder.rs        TextEmbedder trait + Voyage AI implementation
    run_log.rs         RunLogger — JSONB-based operational observability (separate from event store)
    util.rs            URL sanitization, cosine similarity, constants
    agent_tools.rs     Claude API tool schemas for agentic synthesis

  store/
    event_sourced.rs   EventSourcedReader — SignalReader backed by Neo4j graph

  workflows/
    full_run.rs        FullScoutRunWorkflow (Restate durable workflow)
    scrape.rs          ScrapeWorkflow
    bootstrap.rs       BootstrapWorkflow
    synthesis.rs       SynthesisWorkflow
    situation_weaver.rs SituationWeaverWorkflow
    supervisor.rs      SupervisorWorkflow
```

## Trait Boundaries

All external dependencies are injected via async trait objects:

| Trait | Purpose | Production | Test |
|-------|---------|-----------|------|
| `SignalReader` | Read-only graph queries | `EventSourcedReader` (Neo4j) | `MockSignalReader` |
| `ContentFetcher` | Web pages, RSS, social, web search | `Archive` | `MockFetcher` |
| `TextEmbedder` | Text → vector embeddings | `Embedder` (Voyage AI, 1024-dim) | `FixedEmbedder` |
| `SignalExtractor` | Content → structured signals via LLM | `Extractor` (Claude) | `MockExtractor` |

## Two Engine Variants

- **Scrape engine** (`build_engine`): reap → schedule → scrape → enrichment → expansion → synthesis → finalize. Used by scrape and bootstrap workflows.
- **Full engine** (`build_full_engine`): extends the scrape chain with situation_weaving → supervisor before finalize. Used by full_run, synthesis, situation_weaver, and supervisor workflows.

Both share the same `ScoutEngineDeps`, infrastructure handlers, and `PipelineState` aggregate.

## Graph Schema

```
Nodes                          Relationships
─────                          ─────────────
Gathering, Resource,           Signal ──SOURCED_FROM──▶ Citation
HelpRequest, Announcement,     Actor  ──ACTED_IN──▶ Signal
Concern, Condition,            Signal ──RESPONDS_TO──▶ Concern
Citation, Situation,           Situation ──CONTAINS──▶ Signal
Actor, Source, City,           Situation ──EVOLVED_FROM──▶ Situation
Resource, Tag                  Signal ──SIMILAR_TO──▶ Signal
                               Signal ──TAGGED──▶ Tag
                               Signal ──HAS_RESOURCE──▶ Resource
                               Source ──DISCOVERED──▶ Source

Indices
───────
- Vector index (1024-dim per signal type)
- Content hash + URL (dedup)
- Title + type (global dedup)
- Source canonical key
```

## Safety and Quality

| Layer | Mechanism |
|-------|-----------|
| PII detection | Regex patterns for SSN, phone, email, credit card |
| Sensitivity levels | General / Elevated / Sensitive — filtered from public API |
| Geo filtering | Strip fake city-center coords, validate against geo terms |
| Quality scoring | Completeness + geo accuracy → confidence (0.0–1.0) |
| Source diversity | Corroboration from distinct sources increases trust |
| Evidence trail | Every signal linked to citation(s) with retrieval timestamp + content hash |
| Budget limits | Per-operation cost estimation, configurable daily cap |
