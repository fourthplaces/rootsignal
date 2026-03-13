---
title: "refactor: Event sourcing as the system foundation"
type: refactor
date: 2026-02-25
brainstorm: docs/brainstorms/2026-02-25-event-sourcing-brainstorm.md
---

# Event Sourcing as the System Foundation

## Overview

Refactor RootSignal so an append-only event log in Postgres is the single source of truth. Events are **facts about what happened** — infrastructure-agnostic, human-readable, decoupled from any downstream consumer. The Neo4j graph is one consumer: a materialized view rebuilt by a reducer. Other consumers (debugging, AI analysis, future systems) read the same stream.

This is the most grounding piece of the entire infrastructure. Pre-launch is the time to get it right. Every feature built after this inherits auditability, replayability, and retroactive rebalancing for free.

```
Fuzzy world (LLMs, scrapers, embeddings)
         ↓ facts
   Event Log (append-only, universal fact stream)
         ↓                    ↓                ↓
   Graph Reducer         Debugging Tools    AI Analysis
   (events → nodes/edges)
         ↓
   Enrichment passes
   (embeddings, derived metrics, indexes)
         ↓
   Neo4j Graph (complete, queryable)
```

## Core Principle: Events Are Facts, Not Commands

Events describe **what happened in the world**, not what to do to a database. They are completely decoupled from Neo4j, Postgres, or any infrastructure. They stand on their own.

Good: *"A community gathering was discovered at https://... — titled 'Neighborhood Cleanup', starting March 5th, confidence 0.82, corroborated by 3 independent sources"*

Bad: *"MERGE (n:Gathering {id: $id}) SET n.title = $title, n.starts_at = datetime($starts_at)..."*

The reducer's job is to interpret facts into Neo4j operations. The facts don't change if we swap Neo4j for something else. They're readable by humans, queryable by AI, and useful for debugging — not just graph projection.

## One Stream, Not Two

The existing `scout_run_events` table and the new decision events merge into **one unified fact stream**. A URL being scraped is a fact. An LLM extraction is a fact. A signal being created is a fact. A signal being reaped is a fact. They're all things that happened.

The graph reducer processes the facts it cares about (signal_created, signal_corroborated, signal_reaped, etc.) and ignores the rest (scrape_url, llm_extraction, budget_checkpoint). Ignored events are no-ops for the reducer — but they're still valuable for debugging, auditing, and analysis.

This means:
- One table: `events` (replaces `scout_run_events`, absorbs all graph mutation facts)
- One global sequence across everything
- One stream to query for "what happened"
- Multiple consumers that each care about different subsets

## Design Decisions

### 1. Event granularity: Domain-level facts

Events are named by what happened: `signal_discovered`, `signal_corroborated`, `source_scraped`, `review_verdict_reached`, etc. Not generic CRUD. The event log reads like a story of what the system observed and decided.

### 2. Event payload: Facts only, no computed artifacts

Each event carries the factual information at the time of the decision. A `signal_discovered` event carries the signal's title, summary, location, source_url, confidence, dates — everything that was observed. A `signal_corroborated` event carries the corroboration details (which signal, from which new source, at what similarity). No embeddings, no diversity counts, no derived metrics. The event struct shape *is* the guard — if the field isn't in the struct, nobody can accidentally set it.

### 3. The reducer is pure — enrichment is separate

The reducer is a pure function: read a fact, write the corresponding node or edge with factual values only. No API calls, no graph reads, no computation. It doesn't know what an embedding is. It doesn't count edges. It applies facts.

Separate **enrichment passes** run against the reduced graph afterward:

- **Embedding pass** — reads text fields from nodes, computes embeddings (batched), writes them back. Uses a cache keyed by `hash(model_version + input_text)` so subsequent runs are essentially free. Change the embedding model? Cache misses, recomputes everything.
- **Derived metrics pass** — counts SOURCED_FROM edges for `source_diversity` and `channel_diversity`, computes `cause_heat` from graph topology, etc.
- **Future passes** — each new enrichment is its own pass with a single responsibility

```sql
CREATE TABLE embedding_cache (
    input_hash    TEXT PRIMARY KEY,   -- hash(model_version + input_text)
    model_version TEXT NOT NULL,
    embedding     FLOAT4[] NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**Embedding flow during live operation:** Embeddings are computed *before* the event is emitted — they're part of the decision pipeline (dedup requires vector similarity). The pipeline computes the embedding, uses it for dedup, then emits `signal_discovered`. The embedding is passed to the reducer at projection time as a side input — it's not stored in the event (it's not a fact about the world), but it's available for the reducer to write to the Neo4j node. This matches the current flow where embeddings are computed upstream and passed to `create_node()`.

**Embedding flow during replay:** The enrichment pass recomputes embeddings from the signal's text fields after the reducer has projected all facts. The embedding cache makes this fast for subsequent replays.

This means:
- Live writes are fast — embedding already computed, reducer has it in hand
- The reducer is simple — it applies facts and writes the embedding if provided
- Replay works without the embedding service for the reduce step — enrichment handles it afterward
- Embedding cache, batching, model upgrades — all in the enrichment pass
- Each enrichment can be run, skipped, swapped, or improved independently
- Dedup decisions are still captured as facts ("this signal deduplicated against X at similarity 0.93") — we don't re-run dedup on replay

### 4. Infrastructure state: Not in the event stream

These are coordination primitives, not facts about the world:
- `SupervisorLock` / `SupervisorState` — meaningless on replay
- `cached_domain_verdicts` — LLM cache, rebuildable
- `ScoutTask.phase_status` — workflow coordination

These stay as direct writes. They are not facts.

### 5. Bulk operations: Decompose into individual facts

`reap_expired()` first queries which signals match, then emits one fact per signal: "This gathering expired because it's past its end date." Same for `purge_area()` and `delete_by_source_url()`. A single admin action may produce hundreds of facts — that's fine, that's what actually happened.

### 6. Concurrency: Postgres BIGSERIAL with gap-aware cursor

`events.seq` is a `BIGSERIAL` primary key. Multiple producers INSERT concurrently; Postgres assigns sequence numbers atomically. However, BIGSERIAL does not guarantee gap-free commits — transaction A can get seq=100, transaction B gets seq=101, but B commits first. The reducer could see 101 before 100.

**Solution: gap-free reads are a guarantee of the `rootsignal-events` crate.** The `EventStore::read_from()` method never returns events with gaps. It handles this internally — if concurrent transactions created a momentary gap, `read_from()` returns events only up to the gap boundary and the next call picks up where it left off once the gap closes. Consumers never see gaps, never think about gaps, never write gap-handling code. This is the store's job, not the consumer's.

This guarantee is what makes the `rootsignal-events` crate worth existing as a separate module — it encapsulates the hard concurrency problem so every consumer (reducer, admin explorer, AI auditor) gets correct ordering for free.

### 7. CREATE → MERGE with sequence guards: All reducer writes are idempotent

Every signal write in the reducer uses `MERGE (n:Gathering {id: $id})`, not `CREATE`. UUID uniqueness constraints already exist. Replaying the same event twice produces the same graph.

Additionally, every node gets a `last_updated_seq` property set to the event's sequence number. The reducer only applies updates when `event.seq > node.last_updated_seq`. This guards against out-of-order processing, accidental double-processing during crash recovery, and makes the projection robust against future parallelization of the reducer.

### 8. No `now()` in the reducer

Every timestamp comes from the event payload. The reducer uses the fact's timestamp, never wall-clock time. The event captures when something happened; the reducer just records it.

### 9. One event per fact

Each distinct thing that happened is one event. A scout pipeline that discovers a signal + fetches a citation + links an actor = multiple facts. Partial application is acceptable because the reducer is idempotent (MERGE).

### 10. Supervisor bypass: Route through the event stream

`batch_review.rs`, `auto_fix.rs`, and `cause_heat` mutations currently bypass GraphStore and write directly to Neo4j. These must emit facts like everything else. No `client.inner().run(query(...))` backdoors.

### 11. `get_recently_linked_signals_with_queries()`: Refactor

Split into a pure read and a separate fact: "implied queries were consumed for these signals." The read stays as a graph query. The mutation becomes an event.

---

## Event Schema

### `events` table

```sql
CREATE TABLE events (
    seq          BIGSERIAL    PRIMARY KEY,
    ts           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    event_type   TEXT         NOT NULL,
    -- Causal structure
    parent_seq   BIGINT,       -- The event that directly caused this one (tree parent)
    caused_by_seq BIGINT,      -- The root event that initiated this chain (tree root)
    -- Context
    run_id       TEXT,         -- Which scout run, if any
    actor        TEXT,         -- "scout", "supervisor", "admin:<user_id>", "public", "reaper"
    -- The fact itself (JSONB — infrastructure-agnostic, human-readable)
    payload      JSONB        NOT NULL,
    -- Forward compatibility
    schema_v     SMALLINT     NOT NULL DEFAULT 1
);

CREATE INDEX idx_events_type_seq ON events (event_type, seq);
CREATE INDEX idx_events_ts ON events (ts);
CREATE INDEX idx_events_run ON events (run_id) WHERE run_id IS NOT NULL;
CREATE INDEX idx_events_parent ON events (parent_seq) WHERE parent_seq IS NOT NULL;
CREATE INDEX idx_events_caused_by ON events (caused_by_seq) WHERE caused_by_seq IS NOT NULL;
```

**Causal structure:** Events form trees. `parent_seq` points to the direct parent (the event that caused this one). `caused_by_seq` points to the root of the causal chain (the event that started everything). Root events have both fields NULL.

Example chain:
```
seq=100  url_scraped (parent=NULL, caused_by=NULL)        ← root
seq=101  llm_extraction_completed (parent=100, caused_by=100)
seq=102  signal_discovered (parent=101, caused_by=100)
seq=103  citation_recorded (parent=102, caused_by=100)
seq=104  signal_corroborated (parent=102, caused_by=100)  ← same parent, sibling of 103
```

This enables:
- **Tree view:** `WHERE parent_seq = X` → children of event X
- **Full chain:** `WHERE caused_by_seq = X` → everything triggered by event X
- **Flat reduction:** Process by `seq` order (the reducer ignores causal structure)
- **Tree reconstruction:** Group by `caused_by_seq`, sort by `seq`, rebuild the tree

`schema_v` enables forward-compatible evolution. Bump the version when the schema changes. The reducer handles all versions. Old events are never modified.

**Schema evolution policy:** New fields are always additive and `Option<T>` at the event level. The reducer applies `serde(default)` for missing fields in old events. No field is ever renamed or removed — only added. This is the same contract as protobuf wire compatibility.

### Event Types

One unified stream. The graph reducer acts on facts it cares about; the rest are no-ops.

#### Observability Facts (migrated from scout_run_events)

These are already captured today. They move into the unified stream.

| Event Type | Current EventKind | Payload |
|---|---|---|
| `url_scraped` | `ScrapeUrl` | url, strategy, success, content_bytes |
| `feed_scraped` | `ScrapeFeed` | url, items |
| `social_scraped` | `SocialScrape` | platform, identifier, post_count |
| `social_topics_searched` | `SocialTopicSearch` | platform, topics, posts_found |
| `search_performed` | `SearchQuery` | query, provider, result_count, canonical_key |
| `llm_extraction_completed` | `LlmExtraction` | source_url, content_chars, signals_extracted, implied_queries |
| `budget_checkpoint` | `BudgetCheckpoint` | spent_cents, remaining_cents |
| `bootstrap_completed` | `Bootstrap` | sources_created |
| `agent_web_searched` | `AgentWebSearch` | provider, query, result_count, title |
| `agent_page_read` | `AgentPageRead` | provider, url, content_chars, title |
| `agent_future_query` | `AgentFutureQuery` | provider, query, title |

*Graph reducer: no-op on all of these.*

#### Signal Facts (reducer creates/updates/deletes graph nodes)

| Event Type | What happened | Payload |
|---|---|---|
| `signal_discovered` | A new signal was found and validated | Full signal properties + signal_type + source_url (no embedding — computed by reducer) |
| `signal_corroborated` | An existing signal was independently confirmed | signal_id, node_type, new_source_url, similarity, new_corroboration_count (absolute value, computed by producer before append) |
| `signal_refreshed` | A signal's source was re-checked and still active | signal_id(s), node_type, new last_confirmed_active |
| `signal_confidence_scored` | Quality scoring assigned/updated confidence | signal_id, old_confidence, new_confidence |
| `signal_fields_corrected` | Linter auto-corrected signal fields | signal_id, corrections (field → before/after), was_corrected |
| `signal_rejected` | Signal failed quality or review checks | signal_id, reason, source_url |
| `signal_expired` | Signal aged out per retention rules | signal_id, node_type, reason (e.g., "gathering_past_end_date", "need_age_exceeded") |
| `signal_purged` | Admin removed signal (area purge, source deletion) | signal_id, node_type, reason, context |
| `signal_deduplicated` | Signal matched an existing one during dedup | signal_type, title, matched_id, similarity, action, source_url |
| `signal_dropped_no_date` | Signal had no content date and was dropped | title, source_url |
| `review_verdict_reached` | Human or supervisor decided on a signal's status | signal_id, old_status, new_status, reason |
| `implied_queries_consumed` | Expansion queries were extracted and cleared | signal_ids |

#### Citation Facts

| Event Type | What happened | Payload |
|---|---|---|
| `citation_recorded` | Evidence linking a signal to its source | citation properties (url, hash, snippet, relevance, channel_type) + signal_id |
| `orphaned_citations_cleaned` | Cleanup found citations with no parent signal | citation_ids |

#### Source Facts

| Event Type | What happened | Payload |
|---|---|---|
| `source_registered` | A new information source was added | Full source properties |
| `source_updated` | Source metadata changed (weight, active, penalty) | source_id, changed fields |
| `source_deactivated` | Source stopped producing useful signals | source_ids, reason |
| `source_removed` | Source was deleted by admin | source_id, canonical_key |
| `source_scrape_recorded` | Scrape results recorded for a source | canonical_key, signals_produced, scrape_count, consecutive_empty_runs |
| `source_link_discovered` | One source was found via another | child_id, parent_canonical_key |
| `expansion_query_collected` | A signal implied a new search query | query, source_url |
| `expansion_source_created` | An expansion query produced a new source | canonical_key, query, source_url |

#### Actor Facts

| Event Type | What happened | Payload |
|---|---|---|
| `actor_identified` | An actor (person/org) was found in content | Full actor properties |
| `actor_linked_to_signal` | An actor was connected to a signal | actor_id, signal_id, role |
| `actor_linked_to_source` | An actor was connected to a source | actor_id, source_id |
| `actor_stats_updated` | Actor activity metrics changed | actor_id, signal_count, last_active |
| `actor_location_identified` | Actor's location was determined | actor_id, location fields |
| `duplicate_actors_merged` | Auto-fix found and merged duplicate actors | kept_id, merged_ids |
| `orphaned_actors_cleaned` | Cleanup found actors with no connections | actor_ids |

#### Relationship Facts

| Event Type | What happened | Payload |
|---|---|---|
| `relationship_established` | Two entities were linked | from_id, to_id, relationship_type (responds_to, evidence_of, drawn_to, gathers_at, requires, prefers, offers, produced_by), properties |

#### Situation / Dispatch Facts

| Event Type | What happened | Payload |
|---|---|---|
| `situation_identified` | A cluster of signals formed a situation | Full situation properties |
| `situation_evolved` | A situation's state/temperature/narrative changed | situation_id, changed fields |
| `situation_promoted` | All signals in a situation went live | situation_ids |
| `dispatch_created` | A dispatch was generated for a situation | Full dispatch properties |

#### Tag Facts

| Event Type | What happened | Payload |
|---|---|---|
| `tags_aggregated` | Tags were computed for a situation | tag properties, situation_id |
| `tag_suppressed` | A tag was manually removed from a situation | situation_id, tag_slug |
| `tags_merged` | Two tags were consolidated into one | source_slug, target_slug |

#### Quality / Lint Facts

| Event Type | What happened | Payload |
|---|---|---|
| `lint_batch_completed` | Quality check ran on a batch of signals | source_url, signal_count, passed, corrected, rejected |
| `lint_correction_applied` | A signal field was auto-corrected | node_id, signal_type, title, field, old_value, new_value, reason |
| `lint_rejection_issued` | A signal was rejected by the linter | node_id, signal_type, title, reason |
| `empty_signals_cleaned` | Signals with empty titles were removed | signal_ids |
| `fake_coordinates_nulled` | Suspiciously centered coordinates were cleared | signal_ids, old_coords |

#### Other Facts

| Event Type | What happened | Payload |
|---|---|---|
| `schedule_recorded` | A recurring schedule was attached to a signal | schedule properties, signal_id |
| `pin_created` | A user pinned a location | pin properties |
| `pins_removed` | Pins were deleted | pin_ids |
| `demand_signal_received` | A user expressed interest in an area | demand signal properties |
| `demand_aggregated` | Demand signals were batched into scout tasks | created_task_ids, consumed_demand_ids |
| `submission_received` | A public source submission arrived | submission properties, source_canonical_key |

---

## Architecture

### Module Structure

```
modules/
    rootsignal-events/              ← NEW CRATE: generic, zero domain knowledge
        Cargo.toml                  ← depends on: sqlx, serde_json. Nothing else.
        src/
            lib.rs
            store.rs                ← EventStore, EventHandle
            types.rs                ← StoredEvent, AppendEvent, metadata types

    rootsignal-common/              ← domain types
        src/
            events.rs               ← Event enum (SignalDiscovered, UrlScraped, etc.)
                                       Serializes to serde_json::Value for the store.

    rootsignal-graph/               ← graph projection
        src/
            reducer.rs              ← Reads StoredEvent, deserializes Event enum,
                                       projects to Neo4j. Imports both crates.
            enrichment/
                embed.rs            ← Embedding pass (with cache)
                diversity.rs        ← source_diversity, channel_diversity
                cause_heat.rs       ← Graph topology metric

    rootsignal-scout/               ← pipeline (producer)
        src/
            infra/run_log.rs        ← Refactored: uses EventStore + EventHandle
                                       to emit facts with causal chains.

    rootsignal-scout-supervisor/    ← supervisor (producer)
        src/
            checks/batch_review.rs  ← Emits review_verdict_reached via EventStore
            checks/auto_fix.rs      ← Emits cleanup facts via EventStore
```

The `rootsignal-events` crate is completely reusable. It doesn't know what a signal is. It stores opaque JSONB facts with causal structure. The domain types (`Event` enum) live in `rootsignal-common` and serialize at the boundary.

### EventStore

Generic append-only fact store. Lives in its own crate with zero domain knowledge.

```rust
/// Append-only fact store. The single source of truth.
pub struct EventStore {
    pool: PgPool,
}

/// Handle returned by append(). Use to emit child events in the same causal chain.
pub struct EventHandle {
    seq: u64,            // This event's sequence number
    caused_by: u64,      // Root of the causal chain
    store: EventStore,
}

impl EventStore {
    /// Append a root fact (no parent). Returns a handle for emitting children.
    pub async fn append(&self, event: Event) -> Result<EventHandle>;

    /// Read facts in flat sequence order (what the reducer uses).
    pub async fn read_from(&self, seq_start: u64, limit: usize) -> Result<Vec<StoredEvent>>;

    /// Read facts filtered by type (reducer can skip irrelevant events).
    pub async fn read_by_type(&self, event_type: &str) -> Result<Vec<StoredEvent>>;

    /// Read all facts for a given run.
    pub async fn read_by_run(&self, run_id: &str) -> Result<Vec<StoredEvent>>;

    /// Read the full causal tree rooted at an event.
    pub async fn read_tree(&self, root_seq: u64) -> Result<Vec<StoredEvent>>;

    /// Read direct children of an event.
    pub async fn read_children(&self, parent_seq: u64) -> Result<Vec<StoredEvent>>;

    /// The latest sequence number.
    pub async fn latest_seq(&self) -> Result<u64>;
}

impl EventHandle {
    /// Append a child fact caused by this event. Returns a handle for grandchildren.
    pub async fn append(&self, event: Event) -> Result<EventHandle>;

    /// Fire-and-forget: append a child fact, discard the handle.
    pub fn log(&self, event: Event);

    /// This event's sequence number (for referencing as parent).
    pub fn seq(&self) -> u64;
}
```

The `EventHandle` pattern mirrors the existing `RunLogger` / `EventHandle` API — `track()` returns a handle, children nest under it. This preserves the causal tree naturally as events flow through the pipeline. The handle carries `parent_seq` (itself) and `caused_by_seq` (the chain root) so child events inherit the causal context automatically.

### Real-Time Subscriptions (PG NOTIFY)

The event store supports real-time tailing via Postgres LISTEN/NOTIFY. After each INSERT, the store fires `NOTIFY events, '<seq>'` with just the sequence number. Subscribers receive the seq, then pull the full record from the store.

```rust
impl EventStore {
    /// Subscribe to new events in real-time. Returns a stream.
    /// Each notification triggers a fetch of the full StoredEvent by seq.
    pub async fn subscribe(&self) -> Result<impl Stream<Item = StoredEvent>>;

    /// Subscribe with a type filter — only delivers events matching the filter.
    pub async fn subscribe_filtered(
        &self,
        event_types: &[&str],
    ) -> Result<impl Stream<Item = StoredEvent>>;
}
```

**How it works:**
1. INSERT into `events` table
2. Postgres fires `NOTIFY events, '12345'` (just the seq number — well under the 8KB payload limit)
3. Subscriber receives `'12345'`, calls `EventStore::read_event(12345)` to get the full record
4. Full `StoredEvent` delivered to the consumer

**Reliability model:** NOTIFY is a nudge, not a delivery guarantee. If a consumer isn't listening, it misses the notification — but it can always catch up by reading from its last known sequence number. The table is the source of truth. NOTIFY is an optimization that avoids polling.

**This enables:**
- **Async graph reducer** — append is fast, reducer listens and catches up milliseconds later
- **Live admin event explorer** — watch a scout run unfold in real-time
- **AI auditor** — reacts to events as they flow, not just in batch
- **Future webhooks / external consumers** — anything that needs to know "something happened"

This is an optional feature of the `rootsignal-events` crate — consumers opt in to real-time, or poll by sequence number.
```

### GraphReducer

One consumer of the event stream. Projects facts into Neo4j.

```
modules/rootsignal-graph/src/reducer.rs (new)
```

```rust
/// Pure projection of facts into Neo4j nodes and edges.
/// No API calls. No graph reads. No computation.
/// Each fact is either acted upon (MERGE) or ignored (no-op).
/// Uses last_updated_seq guards for idempotency.
pub struct GraphReducer {
    graph: GraphClient,
}

impl GraphReducer {
    /// Apply a single fact to the graph. Idempotent (MERGE + seq guard).
    /// Returns true if the fact produced a graph change, false if no-op.
    pub async fn apply(&self, event: &StoredEvent) -> Result<bool>;

    /// Replay: apply all facts from seq_start in order.
    pub async fn replay_from(&self, store: &EventStore, seq_start: u64) -> Result<u64>;

    /// Full rebuild: wipe graph, replay all facts from the beginning.
    /// Uses UNWIND batching for performance — groups consecutive same-type
    /// events and applies them in batch Cypher operations.
    pub async fn rebuild(&self, store: &EventStore) -> Result<u64>;
}
```

### Enrichment Passes

Separate processes that run after the reducer to compute derived values.

```
modules/rootsignal-graph/src/enrichment/ (new directory)
    embed.rs      — compute + cache embeddings for nodes missing them
    diversity.rs  — recompute source_diversity, channel_diversity from SOURCED_FROM edges
    cause_heat.rs — recompute cause_heat from graph topology
```

```rust
/// Compute embeddings for all signal nodes that need them.
/// Batches API requests. Uses embedding_cache for efficiency.
pub struct EmbeddingEnricher {
    graph: GraphClient,
    embedder: EmbeddingClient,
    cache: EmbeddingCache,
}

impl EmbeddingEnricher {
    /// Enrich all nodes missing embeddings or with stale model versions.
    pub async fn run(&self) -> Result<EnrichmentStats>;
}

/// Recompute derived metrics from graph topology.
pub struct DerivedMetricsEnricher {
    graph: GraphClient,
}

impl DerivedMetricsEnricher {
    /// Recompute source_diversity, channel_diversity, cause_heat.
    pub async fn run(&self) -> Result<EnrichmentStats>;
}
```

### Event enum

```
modules/rootsignal-common/src/events.rs (new)
```

Typed Rust enum with serde Serialize/Deserialize. Each variant carries the complete fact. Variants are named as facts about the world, not as database operations.

### GraphStore refactor

`writer.rs` methods become thin wrappers:
1. Compute the decision (same logic as today)
2. Build an `Event` variant describing what happened
3. Append to `EventStore`
4. `GraphReducer::apply()` projects the fact into Neo4j nodes/edges
5. Enrichment passes run after (embedding, derived metrics)

The writer no longer contains Cypher queries. All Cypher lives in the reducer. All computed values live in enrichment passes.

### RunLogger migration

`run_log.rs` migrates from writing to `scout_run_events` to writing to the unified `events` table via `EventStore::append()`. The `RunLogger` API stays the same — `track(kind)` and `log(kind)` — but the backing store changes. The `EventKind` enum merges into the unified `Event` enum.

### Existing code changes

| File | Change |
|---|---|
| `rootsignal-events/` | **New crate:** EventStore, EventHandle, StoredEvent. Generic, no domain knowledge. |
| `rootsignal-common/src/events.rs` | New: unified Event enum (~55 variants, facts only, no derived fields) |
| `rootsignal-graph/src/reducer.rs` | New: GraphReducer (pure: events → nodes/edges, no API calls) |
| `rootsignal-graph/src/enrichment/` | New: embed.rs, diversity.rs, cause_heat.rs (post-reduce passes) |
| `rootsignal-graph/src/writer.rs` | Refactor: methods build Events + call EventStore + GraphReducer + enrichment |
| `rootsignal-scout/src/infra/run_log.rs` | Refactor: write to unified events table via EventStore + EventHandle |
| `rootsignal-scout-supervisor/src/checks/batch_review.rs` | Refactor: emit events instead of direct Neo4j writes |
| `rootsignal-scout-supervisor/src/checks/auto_fix.rs` | Refactor: emit events instead of direct Neo4j writes |
| `rootsignal-graph/src/reader.rs` | Unchanged — reads don't produce events |
| `rootsignal-graph/src/migrate.rs` | Unchanged — one-time schema migrations are not runtime facts |
| `rootsignal-api/src/graphql/mutations.rs` | Verify all paths go through writer (most already do) |

---

## Implementation Phases

### Phase 1: rootsignal-events crate + Event enum

**Goal:** The `rootsignal-events` crate exists as a standalone, domain-agnostic event store. The `events` table exists. The domain `Event` enum is defined in `rootsignal-common`. Existing `scout_run_events` data is migrated.

**Files:**
- `modules/rootsignal-events/Cargo.toml` — new crate (sqlx, serde_json, uuid, chrono)
- `modules/rootsignal-events/src/lib.rs` — exports
- `modules/rootsignal-events/src/store.rs` — EventStore, EventHandle
- `modules/rootsignal-events/src/types.rs` — StoredEvent, AppendEvent (generic, no domain types)
- `modules/rootsignal-api/migrations/0XX_unified_events.sql` — CREATE TABLE events with parent_seq, caused_by_seq
- `modules/rootsignal-common/src/events.rs` — unified `Event` enum (~55 domain-specific variants)

**Acceptance criteria:**
- [x] `rootsignal-events` crate compiles with zero domain dependencies
- [x] `events` table exists with BIGSERIAL seq, parent_seq, caused_by_seq, JSONB payload, schema_v
- [x] `EventStore::append()` returns `EventHandle` for causal chaining
- [x] `EventHandle::append()` correctly sets parent_seq and caused_by_seq
- [x] `read_from()`, `read_tree()`, `read_children()` all work
- [x] All ~55 `Event` variants defined in rootsignal-common with fact-oriented naming
- [x] Events round-trip through serde (enum → JSONB → enum)
- [ ] Existing scout_run_events data migrated to new table

### Phase 2: GraphReducer (pure)

**Goal:** A pure reducer that applies facts to Neo4j as nodes and edges. No API calls, no graph reads, no computation. MERGE-based, idempotent.

**Files:**
- `modules/rootsignal-graph/src/reducer.rs` — `GraphReducer` with `apply()` matching all ~55 variants (~30 produce graph changes, ~25 are no-ops)
- All Cypher queries move from writer.rs into the reducer

**Acceptance criteria:**
- [x] Every graph-mutating Event variant has a MERGE-based Cypher handler
- [x] Observability events (url_scraped, llm_extraction_completed, etc.) are explicit no-ops
- [x] No `Utc::now()`, no `Uuid::new_v4()`, no API calls, no graph reads in reducer
- [x] Reducer writes only factual values — no embeddings, no diversity counts, no cause_heat
- [x] `GraphReducer::replay_from()` reads events in order and applies each
- [x] `GraphReducer::rebuild()` wipes graph and replays from seq 0

### Phase 2b: Enrichment passes

**Goal:** Separate processes that compute derived values on the reduced graph.

**Files:**
- `modules/rootsignal-graph/src/enrichment/embed.rs` — compute + cache embeddings
- `modules/rootsignal-graph/src/enrichment/diversity.rs` — recompute source_diversity, channel_diversity
- `modules/rootsignal-graph/src/enrichment/cause_heat.rs` — recompute cause_heat

**Acceptance criteria:**
- [ ] Embedding enricher computes embeddings for nodes missing them, using cache
- [ ] Embedding enricher batches API requests for performance
- [ ] Diversity enricher recomputes counts from SOURCED_FROM edges
- [ ] Each enricher is independently runnable and idempotent
- [ ] After reducer + all enrichers, graph matches expected state

### Phase 3: Wire writer.rs through EventStore + GraphReducer

**Goal:** Every GraphStore method emits a fact to the event store, then the reducer projects it into Neo4j. Direct Cypher removed from writer.rs. A catch-up loop guarantees eventual consistency.

**Files:**
- `modules/rootsignal-graph/src/writer.rs` — refactor all ~70 methods
- All CREATE → MERGE conversion happens in the reducer

**Strategy:** Method by method:
1. Build the `Event` variant with all computed values
2. `event_store.append(event)` → fact is persisted
3. `reducer.apply(event)` → fact is projected to Neo4j (inline, fast path)
4. Remove old direct Cypher from the method

**Catch-up loop (required from day one):** A periodic background task (e.g., every 30 seconds) compares the reducer's `last_processed_seq` with `EventStore::latest_seq()`. If the graph is behind (inline reduce failed, process crashed, Neo4j was briefly down), the catch-up loop replays the gap. The inline reducer is a performance optimization. The catch-up loop is the correctness guarantee. Both must exist from Phase 3.

**Acceptance criteria:**
- [ ] All GraphStore methods produce events
- [ ] All signal CREATE in reducer converted to MERGE
- [ ] `events` table populates during scout runs
- [ ] Existing tests pass — graph state identical to pre-refactor
- [ ] Catch-up loop runs periodically and replays any events the inline reducer missed

### Phase 4: Migrate RunLogger to unified stream

**Goal:** `run_log.rs` writes to the `events` table instead of `scout_run_events`. One stream for everything.

**Files:**
- `modules/rootsignal-scout/src/infra/run_log.rs` — swap backing store to EventStore + EventHandle
- `modules/rootsignal-api/migrations/0XX_drop_scout_run_events.sql` — drop old table (after migration)

**`scout_runs` table:** Stays as infrastructure/coordination (stores computed stats, finished_at). Drop the FK to `scout_run_events`. The `run_id` field on the unified `events` table replaces the FK relationship.

**Acceptance criteria:**
- [ ] RunLogger writes to `events` table via EventStore + EventHandle (preserving causal chains)
- [ ] Observability events (url_scraped, llm_extraction_completed, etc.) appear in unified stream
- [ ] `has_event_type()` test helper still works
- [ ] `scout_run_events` table dropped
- [ ] `scout_runs` FK to `scout_run_events` dropped; `scout_runs` stays for stats

### Phase 5: Supervisor + admin bypass elimination

**Goal:** No more `client.inner()` bypasses. All graph mutations flow through events.

**Files:**
- `modules/rootsignal-scout-supervisor/src/checks/batch_review.rs` — emit events via EventStore
- `modules/rootsignal-scout-supervisor/src/checks/auto_fix.rs` — emit events via EventStore
- Refactor `get_recently_linked_signals_with_queries()` — split read from mutation

**Acceptance criteria:**
- [ ] `batch_review.rs` emits `review_verdict_reached` events
- [ ] `auto_fix.rs` emits cleanup events
- [ ] No remaining `client.inner()` calls for domain data mutations
- [ ] `get_recently_linked_signals_with_queries()` split into read + `implied_queries_consumed` event

### Phase 6: Bulk operation decomposition

**Goal:** `reap_expired()`, `purge_area()`, `delete_by_source_url()` emit individual facts per affected node.

**Strategy:** Each bulk method:
1. Query which nodes match (read-only)
2. For each match, emit a fact ("this signal expired because...")
3. Reducer handles the DETACH DELETE

**Acceptance criteria:**
- [ ] `reap_expired()` emits individual `signal_expired` events with UUIDs and reasons
- [ ] `purge_area()` emits individual `signal_purged` events
- [ ] `delete_by_source_url()` emits individual events
- [ ] Cascade deletes handled by reducer's DETACH DELETE

### Phase 7: Replay verification test

**Goal:** Prove the event log is complete. Replay from scratch produces the same graph.

**Files:**
- `modules/rootsignal-graph/tests/replay_test.rs`

**Strategy:**
1. Run scout pipeline against test data (testcontainers Neo4j + Postgres)
2. Snapshot the graph (all nodes + edges + properties)
3. Wipe Neo4j
4. `GraphReducer::rebuild()` from the event stream
5. Snapshot again
6. Diff

**What "same graph" means (reducer test — factual values only):**
- All nodes with same UUIDs and same factual property values
- All edges between same node pairs with same properties
- Excluded from reducer diff: embeddings, source_diversity, channel_diversity, cause_heat (all computed by enrichment passes, not the reducer)
- Excluded: infrastructure nodes (SupervisorLock)

**Enrichment test (separate):**
- After reducer + enrichment passes, embeddings exist on all signal nodes
- Diversity counts match expected values
- cause_heat values are non-zero where expected

**Acceptance criteria:**
- [ ] Replay test passes
- [ ] Runs in CI with testcontainers
- [ ] Covers: signal discovery, corroboration, reaping, review verdicts, actor linking, citations, situations

### Phase 8: Cleanup

**Goal:** Remove dead code. Verify full pipeline: events → reducer → enrichment → queryable graph.

**Acceptance criteria:**
- [ ] No dead direct-write code remaining in writer.rs
- [ ] No `client.inner()` bypasses anywhere
- [ ] Full pipeline test: append events → reduce → enrich → query → correct results
- [ ] Replay test passes (reducer only, no enrichment needed)
- [ ] Enrichment test passes (reducer + enrichment = complete graph)

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Enrichment needs embedding service | Embedding pass requires API access | Embedding cache makes subsequent runs free. Pre-warm from live graph. Reducer itself needs no external services — replay test is pure. |
| Cold rebuild embedding cost | First embedding pass after model upgrade is slow | Batch API requests. Pre-warm cache. Cold rebuilds are rare — only on model upgrades. |
| Reducer bugs cause graph divergence | Wrong graph state | Replay test catches this. Run after every deploy. |
| Missing event type | Incomplete audit trail | Replay test fails → add the missing event → replay |
| MERGE performance vs CREATE | Slower signal writes | MERGE on UUID with uniqueness constraint is fast. Benchmark. |
| RunLogger migration breaks observability | Debugging blind spot | Migrate carefully. Keep `has_event_type()` helper working. |
| Unified table grows large | Query performance | Partition by event_type or ts if needed. BIGSERIAL handles billions of rows. |

## Event Stream Consumers

The event stream is a universal fact log. Multiple consumers read the same stream for different purposes.

### Graph Reducer (implemented in this plan)
Reads events in flat sequence order. Projects facts into Neo4j nodes and edges. Ignores events it doesn't care about (observability facts are no-ops). Followed by enrichment passes.

### Admin Event Explorer (future)
Displays the event tree in the admin app. Uses `read_tree(root_seq)` and `read_children(parent_seq)` to render the causal hierarchy. For any signal in the graph, trace back through the tree: signal_discovered ← llm_extraction_completed ← url_scraped ← bootstrap. The full paper trail, visualized.

### AI Auditor (future)
Another consumer that reads the event stream and looks for anomalies: why are we scraping this domain 50 times with no signals? Why did this source suddenly 10x its output? Why were 5 corroborating sources all registered in the same hour? The immune system applied to the system's own behavior.

### Tree ↔ Sequence Duality
The same data supports two views:
- **Sequence** (flat, ordered by `seq`) — what the reducer processes
- **Tree** (causal, structured by `parent_seq` / `caused_by_seq`) — what humans and AI auditors inspect

Flatten: depth-first traversal of the tree = the sequence.
Expand: group by `caused_by_seq`, sort by `seq`, reconstruct the tree.

## Open Questions (to resolve during implementation)

- **Event retention:** Keep forever for now. Revisit compaction/snapshotting if table exceeds 10M rows.
- **Reducer as separate process:** Start inline. Extract to separate binary only if horizontal scaling demands it.
- **Async reducer:** Should the reducer run as a NOTIFY subscriber instead of inline? Start inline, switch to async if append latency matters.

## References

- Brainstorm: `docs/brainstorms/2026-02-25-event-sourcing-brainstorm.md`
- Current event types: `modules/rootsignal-scout/src/infra/run_log.rs`
- Graph writer: `modules/rootsignal-graph/src/writer.rs`
- Supervisor bypass: `modules/rootsignal-scout-supervisor/src/checks/batch_review.rs`
- Data quality learning: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
- Pipeline architecture: `docs/architecture/scout-pipeline.md`
- Feedback loops: `docs/architecture/feedback-loops.md`
