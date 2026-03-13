# Thin Handlers / Pure Activities Refactor

Two rules:
1. **Handlers are thin** — extract event fields, call activity, map result to events
2. **Activities are pure-ish** — return domain types, never seesaw `Events`. Events are constructed *in* handlers.

Activities can take `&ScoutEngineDeps` for ergonomics (store, ai, graph access), but should not import or return `seesaw_core::Events`.

---

## Phase 1: Enrichment

Easiest domain — activities are small and the pattern is clear.

### 1a. `activities/diversity.rs`
- **Now:** `compute_diversity_events()` → `seesaw_core::Events`
- **After:** `compute_diversity_scores()` → `Vec<SignalDiversityScore>`
- Handler wraps result in `SystemEvent::SignalDiversityComputed { metrics }` + `EnrichmentEvent::DiversityScored`

### 1b. `activities/actor_stats.rs`
- **Now:** `compute_actor_stats_events()` → `seesaw_core::Events`
- **After:** `compute_actor_stats()` → `Vec<ActorStatScore>`
- Handler wraps result in `SystemEvent::ActorStatsComputed { stats }` + `EnrichmentEvent::ActorStatsComputed`

### 1c. `activities/actor_location.rs`
- **Now:** `triangulate_actor_location_events()` → `seesaw_core::Events`
- **After:** `triangulate_all_actors()` → `Vec<ActorLocationUpdate>` (new struct: actor_id, lat, lng, name)
- Handler maps each to `SystemEvent::ActorLocationIdentified` + `EnrichmentEvent::ActorsLocated`
- Delete `enrich_actor_locations()` (dead code — uses engine.emit directly, bypassing handler pattern)

### 1d. `activities/actor_extractor.rs`
- **Now:** `run_actor_extraction()` → `(ActorExtractorStats, seesaw_core::Events)`
- **After:** `run_actor_extraction()` → `ActorExtractionResult` containing:
  - `stats: ActorExtractorStats`
  - `new_actors: Vec<NewActor>` (id, name, type, canonical_key, lat, lng)
  - `actor_links: Vec<ActorLink>` (actor_id, signal_id, role)
- Handler maps `new_actors` → `SystemEvent::ActorIdentified`, `actor_links` → `SystemEvent::ActorLinkedToSignal`

### 1e. `mod.rs` — `run_enrichment` handler
- Currently 80 lines of inline orchestration
- After: same structure but each step calls activity → maps result → pushes events
- No logic changes, just cleaner event construction at handler boundary

### 1f. `activities/compute_source_metrics` (in enrichment/activities/mod.rs)
- **Now:** returns `seesaw_core::Events`
- **After:** returns `Vec<SourceMetric>` (new struct)
- Handler maps to `SystemEvent::SourceMetricsComputed`

---

## Phase 2: Discovery

### 2a. `activities/bootstrap.rs` — `seed_sources_if_empty()`
- **Now:** returns `Result<seesaw_core::Events>`, constructs `DiscoveryEvent::SourcesDiscovered`
- **After:** returns `Result<Vec<SourceNode>>`
- Handler wraps in `DiscoveryEvent::SourcesDiscovered` if non-empty

### 2b. `activities/domain_filter_gate.rs` — `filter_discovered_sources()`
- **Now:** returns `seesaw_core::Events` (SourcesRegistered/rejected events)
- **After:** returns `FilterResult { accepted: Vec<SourceNode>, rejected: Vec<(SourceNode, String)> }`
- Handler maps to `DiscoveryEvent::SourcesRegistered` + rejection logging events

### 2c. `activities/source_expansion.rs` / `discover_expansion_sources()`
- **Now:** returns struct with `events: seesaw_core::Events`
- **After:** returns struct without events field; handler maps `social_topics` → `DiscoveryEvent::SocialTopicsDiscovered`

### 2d. `activities/link_promotion.rs`
- Check if `into_events()` returns seesaw Events — if so, same treatment

---

## Phase 3: Expansion

### 3a. `activities/expansion.rs` — `ExpansionOutput`
- **Now:** struct contains `events: seesaw_core::Events`
- **After:** struct contains `consumed_signal_ids: Vec<Uuid>` (for ImpliedQueriesConsumed), `query_embeddings: Vec<(String, Vec<f64>)>` (for QueryEmbeddingStored)
- Handler maps these to `SystemEvent::ImpliedQueriesConsumed` and `SystemEvent::QueryEmbeddingStored`

### 3b. `mod.rs` — `expand_signals` handler
- Currently builds ScrapeEvent::SourcesResolved + TopicDiscoveryCompleted inline
- After: activity returns topic scrape data as plain struct, handler constructs events

---

## Phase 4: Synthesis

### 4a. `activities/response_mapper.rs`
- **Now:** `map_single_tension()` → `(Events, u32)`, `map_responses()` takes `&mut Events`
- **After:** `map_single_tension()` → `Vec<ResponseLink>` (signal_id, concern_id, strength, explanation)
- Handler maps each to `SystemEvent::ResponseLinked`
- Delete `map_responses()` — the handler already does the parallelization

### 4b. `mod.rs` — `compute_similarity` handler
- Already nearly thin — calls `rootsignal_graph::similarity::compute_edges()` which returns plain `Vec<SimilarityEdge>`
- Just clean up: result is already domain types, handler already constructs events. Minimal change.

### 4c. `mod.rs` — `infer_severity` handler
- Calls `rootsignal_graph::severity_inference::compute_severity_inference()` which returns `Vec<SystemEvent>`
- This is in a different crate — may defer changing the graph crate's return type

---

## Phase 5: Signals (Dedup)

Most complex. `dedup.rs` is 560 lines that interleave dedup logic with event construction.

### 5a. Extract event construction from `deduplicate_extracted_batch()`
- **Now:** builds `WorldEvent`, `SystemEvent`, `CitationPublished`, `SignalEvent::DedupCompleted` inline
- **After:** returns `DedupResult`:
  - `created: Vec<CreatedSignal>` (node, author, resource_tags, signal_tags, source_id)
  - `corroborated: Vec<CorroboratedSignal>` (existing_id, node_type, url, similarity, count, content_hash)
  - `refreshed: Vec<RefreshedSignal>` (existing_id, node_type, url)
  - `verdicts: Vec<DedupOutcome>` (kept for pipeline state)
- Handler maps:
  - `created` → `WorldEvent` + `SystemEvent`s + `CitationPublished` + actor resolution events
  - `corroborated` → `CitationPublished` + `ObservationCorroborated` + `CorroborationScored`
  - wraps all in `SignalEvent::DedupCompleted`

### 5b. `resolve_actor_inline()` — currently takes `&mut Events`
- **After:** returns `Option<(ResolvedActor, Vec<ActorAction>)>` where `ActorAction` is enum:
  - `Create { id, name, type, canonical_key, ... }`
  - `Link { actor_id, signal_id, role }`
  - `LinkToSource { actor_id, source_id }`
- Handler maps actions to events

### 5c. `build_corroboration()` — currently returns `(Events, DedupOutcome)`
- **After:** returns `(CorroborationData, DedupOutcome)` where `CorroborationData` has the fields needed for CitationPublished + ObservationCorroborated + CorroborationScored

---

## Phase 6: Curiosity (Investigator)

### 6a. `Investigator::run()` — takes `&mut seesaw_core::Events`
- **After:** returns `(InvestigationStats, Vec<InvestigationAction>)` where actions are:
  - `EvidenceFound { citation, signal_id }`
  - `ConfidenceRevised { signal_id, old, new }`
  - `SignalInvestigated { signal_id, node_type, at }`
- Handler maps to `WorldEvent::CitationPublished`, `SystemEvent::ConfidenceScored`, `SystemEvent::SignalInvestigated`

### 6b. `investigate_single_signal()` — returns `(seesaw_core::Events, InvestigationTargetStats)`
- Same treatment: return domain types, handler maps to events

### 6c. `investigate_signal()` / `compute_confidence_revision()` — take `&mut Events`
- Internals of investigator — return data, don't push events

---

## Execution Notes

- Each phase is independently shippable
- No behavioral changes — same events emitted, same order
- Tests: existing tests should pass since they test at the handler level (MOCK → FUNCTION → OUTPUT)
- Phase 1 is the proof of concept — smallest, clearest, builds the pattern
- Phase 5 (dedup) is the hardest — largest file, most interleaved logic. Do last.
