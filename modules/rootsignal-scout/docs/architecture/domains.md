# Domains

Each domain is a self-contained module with its own events, handlers, and activity functions. Handlers react to events, perform work via activity functions, and emit new events. No domain directly mutates shared state — all state changes flow through the aggregate.

## Lifecycle

**Purpose**: Pipeline orchestration — start, phase transitions, finalize.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `lifecycle:reap` | `EngineStarted` | `SystemEvent::EntityExpired`, `PhaseCompleted(ReapExpired)` |
| `lifecycle:schedule` | `PhaseCompleted(ReapExpired)` | `PipelineEvent::ScheduleResolved`, `SourcesScheduled` |
| `lifecycle:finalize` (scrape) | `PhaseCompleted(Synthesis)` | `RunCompleted` |
| `lifecycle:finalize` (full) | `PhaseCompleted(Supervisor)` | `RunCompleted` |

**Activities**: `reap_expired()` queries the graph for signals past TTL. `schedule_sources()` loads sources, computes cadence-based scheduling, partitions into tension/response phases, resolves actor contexts and URL mappings.

**Key design**: The schedule handler emits `PipelineEvent::ScheduleResolved` to stash scheduling data in the aggregate, then `SourcesScheduled` to trigger scraping. This two-event pattern separates state mutation from phase signaling.

---

## Scrape

**Purpose**: Fetch web content, social posts, and extract signals via LLM.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `scrape:tension` | `SourcesScheduled` | `ScrapeEvent::*`, `SignalEvent::SignalsExtracted`, `PipelineEvent::ScrapeAccumulated`, `PhaseCompleted(TensionScrape)` |
| `scrape:response` | `PhaseCompleted(SourceExpansion)` | Same pattern + `PipelineEvent::SocialTopicsConsumed`, `PhaseCompleted(ResponseScrape)` |

**Activities**: `ScrapePhase` drives the web scraping pipeline:
1. Deduplicate URLs across sources
2. Fetch via headless Chrome (10 concurrent, 30s timeout)
3. Content hash check (skip unchanged pages)
4. LLM extraction (Claude) → structured signals
5. Emit `SignalEvent::SignalsExtracted` per URL (triggers dedup sub-chain)

Social scraping runs in parallel via Apify (Instagram, Facebook, Reddit).

**Key design**: Scrape handlers emit `PipelineEvent::ScrapeAccumulated` to fold scrape output (URL mappings, signal counts, pub dates, collected links) into the aggregate, replacing direct state writes.

---

## Signals

**Purpose**: Deduplication, signal creation, and edge wiring — the signal processing sub-chain.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `signals:dedup` | `SignalEvent::SignalsExtracted` | `NewSignalAccepted`, `CrossSourceMatchDetected`, `SameSourceReencountered`, `DedupCompleted`, `UrlProcessed` |
| `signals:create` | `NewSignalAccepted` | `WorldEvent::*` (signal creation), `CitationPublished`, `SensitivityClassified`, `SignalCreated` |
| `signals:corroborate` | `CrossSourceMatchDetected` | `CitationPublished`, `ObservationCorroborated`, `CorroborationScored` |
| `signals:refresh` | `SameSourceReencountered` | `CitationPublished`, `FreshnessConfirmed` |
| `signals:wire_edges` | `SignalCreated` | `SignalLinkedToSource`, `ResourceLinked`, `ActorIdentified`, `ActorLinkedToSignal`, `SignalTagged` |

**Activities**: `deduplicate_extracted_batch()` implements the 4-layer dedup pipeline (see [dedup-pipeline.md](dedup-pipeline.md)). `create_signal_events()` maps accepted signals to World/System events. `wire_signal_edges()` creates all relationship edges.

**Key design**: The dedup handler stashes `PendingNode` data in the aggregate via `NewSignalAccepted`. The create handler reads it, creates World events, and stashes `WiringContext`. The wire_edges handler reads that context and creates relationship edges. Each handler has a clear lifecycle: stash → consume → clear.

---

## Discovery

**Purpose**: Source finding, link promotion, bootstrapping.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `discovery:bootstrap` | `EngineStarted` | `DiscoveryEvent::SourceDiscovered` (seed sources) |
| `discovery:link_promotion` | `PhaseCompleted(TensionScrape\|ResponseScrape\|SignalExpansion)` | `SourceDiscovered`, `LinksPromoted` |
| `discovery:source_expansion` | `PhaseCompleted(TensionScrape)` | `SourceDiscovered`, `ExpansionQueryCollected`, `SocialTopicCollected`, `SocialTopicsCollected`, `PhaseCompleted(SourceExpansion)` |

**Activities**: `seed_sources_if_empty()` populates initial sources for a new region. `SourceFinder` uses Claude to analyze graph gaps and propose new sources. Link promotion converts discovered links (collected during scraping) into first-class Source nodes.

**Key design**: `DiscoveryEvent::SourceDiscovered` is the only domain event that is projected to Neo4j (via `is_projectable()`). All other discovery events are aggregate-only bookkeeping.

---

## Enrichment

**Purpose**: Actor extraction, location triangulation, quality scoring, source metrics.

**Handlers** (parallel fan-out on `PhaseCompleted(ResponseScrape)`):

| ID | Trigger | Emits |
|----|---------|-------|
| `enrichment:actor_extraction` | `PhaseCompleted(ResponseScrape)` | `SystemEvent::PinsConsumed`, actor events, `EnrichmentRoleCompleted(ActorExtraction)` |
| `enrichment:diversity` | `PhaseCompleted(ResponseScrape)` | `EnrichmentEvent::SignalDiversityComputed`, `EnrichmentRoleCompleted(Diversity)` |
| `enrichment:actor_stats` | `PhaseCompleted(ResponseScrape)` | `EnrichmentEvent::ActorStatsComputed`, `EnrichmentRoleCompleted(ActorStats)` |
| `enrichment:actor_location` | `PhaseCompleted(ResponseScrape)` | `SystemEvent::ActorLocationIdentified`, `EnrichmentRoleCompleted(ActorLocation)` |
| `enrichment:phase_complete` | `EnrichmentRoleCompleted` | `PhaseCompleted(ActorEnrichment)` (when all 4 roles done) |
| `enrichment:metrics` | `PhaseCompleted(ActorEnrichment)` | `SystemEvent::SourceChanged`, `SourceScraped`, `MetricsCompleted` |

**Activities**: `triangulate_actor_location_events()` geolocates actors from signal evidence. `run_actor_extraction()` handles actor extraction via LLM. `compute_diversity_events()` computes evidence diversity per signal. `compute_actor_stats_events()` computes per-actor statistics. `compute_source_metrics()` updates source weights and cadences based on signal yield.

---

## Signal Expansion

**Purpose**: Follow implied queries discovered during extraction to find additional signals.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `expansion:signal_expansion` | `MetricsCompleted` | `DiscoveryEvent::SourceDiscovered`, `PipelineEvent::ExpansionAccumulated`, `PhaseCompleted(SignalExpansion)` |

**Activities**: `expand_and_discover()` collects implied queries from high-value signals, deduplicates them, creates new expansion sources, and runs end-of-run discovery via `SourceFinder`.

---

## Synthesis

**Purpose**: Cross-signal relationship discovery via 6 parallel seesaw handlers.

**Architecture**: The synthesis phase uses a trigger → fan-out → completion pattern. A single trigger handler emits `SynthesisTriggered`, which fires 6 independent role handlers in parallel. Each role handler emits `SynthesisRoleCompleted` when done. A completion handler checks whether all 6 roles are finished (via `completed_synthesis_roles` in `PipelineState`) and emits `PhaseCompleted(Synthesis)`.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `synthesis:trigger` | `PhaseCompleted(SignalExpansion)` | `SynthesisTriggered` |
| `synthesis:similarity` | `SynthesisTriggered` | `SystemEvent::SimilarityEdgesRebuilt`, `SynthesisRoleCompleted(Similarity)` |
| `synthesis:response_mapping` | `SynthesisTriggered` | `SystemEvent::ResponseLinked`, `SynthesisRoleCompleted(ResponseMapping)` |
| `synthesis:tension_linker` | `SynthesisTriggered` | `SystemEvent::ConcernLinked`, `SynthesisRoleCompleted(ConcernLinker)` |
| `synthesis:response_finder` | `SynthesisTriggered` | `WorldEvent::ResourceLinked`, `DiscoveryEvent::SourceDiscovered`, `SynthesisRoleCompleted(ResponseFinder)` |
| `synthesis:gathering_finder` | `SynthesisTriggered` | `WorldEvent::GatheringAnnounced`, `DiscoveryEvent::SourceDiscovered`, `SynthesisRoleCompleted(GatheringFinder)` |
| `synthesis:investigation` | `SynthesisTriggered` | `SystemEvent::ObservationCorroborated`, `SynthesisRoleCompleted(Investigation)` |
| `synthesis:phase_complete` | `SynthesisRoleCompleted` | `PhaseCompleted(Synthesis)` (when all 6 complete) |
| `synthesis:severity_inference` | `PhaseCompleted(Synthesis)` | `SystemEvent::SeverityClassified` |

**Roles**:
- **Similarity**: Rebuilds cosine-similarity edges between signals in the graph
- **Response Mapping**: LLM determines which Resource/Gathering signals address HelpRequest/Concern signals → `RESPONDS_TO` edges
- **Concern Linker**: Agentic search linking orphaned signals to existing concerns
- **Response Finder**: Agentic investigation discovering ecosystem responses to top concerns
- **Gathering Finder**: Agentic investigation discovering physical gathering places around concerns
- **Investigator**: Web search corroboration for low-confidence signals

**Shared utilities** (`synthesis/util.rs`): Region bounding box computation, future query `SourceNode` builder, `NodeMeta` construction, best-tension-match via embedding similarity. These are shared by response_finder and gathering_finder to avoid duplication.

All synthesis roles are budget-gated — they check `has_budget()` before running. If deps are missing (region, graph, budget, archive), the trigger handler skips synthesis entirely.

---

## Situation Weaving

**Purpose**: Cluster signals into situations, generate narratives, trigger curiosity.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `situation_weaving:run` | `PhaseCompleted(Synthesis)` | `SystemEvent::SituationIdentified/Changed/Promoted`, `CuriosityTriggered`, `SourcesBoostedForSituation`, `PhaseCompleted(SituationWeaving)` |

**Activities**: `weave_situations()` builds similarity edges between signals, runs Leiden community detection to cluster them into situations, uses LLM to generate situation titles and summaries, computes situation metrics (energy, velocity, arc), and boosts source weights for hot situations.

*Only registered in the full engine.*

---

## Supervisor

**Purpose**: Quality control — issue detection, duplicate merging, cause heat, beacons.

**Handlers**:

| ID | Trigger | Emits |
|----|---------|-------|
| `supervisor:run` | `PhaseCompleted(SituationWeaving)` | `SystemEvent::DuplicateConcernMerged`, `DuplicateActorsMerged`, `PhaseCompleted(Supervisor)` |

**Activities**: `supervise()` detects and merges duplicate tensions, merges duplicate actors, computes cause heat (cross-situation attention spillover), and identifies beacon signals (high-impact outliers).

*Only registered in the full engine.*

---

## Scheduling (Utility)

**Purpose**: Budget tracking, source scheduling algorithms. No handlers — pure utility functions used by other domains.

**Components**:
- `BudgetTracker`: Enforces configurable daily spend limit. Each LLM call, web search, and API operation has an estimated cost.
- `Metrics`: Source weight and cadence computation.
- `Scheduler`: Cadence-based source scheduling with exploration slots (10% random sampling of stale sources).
