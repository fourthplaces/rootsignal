# Event Flow

The complete causal chain from `EngineStarted` to `RunCompleted`. Each indentation level represents child events emitted by the parent handler.

## Full Engine Chain

```
 EngineStarted { run_id }
│
├─[lifecycle:reap]
│   └─ SystemEvent::EntityExpired (per expired signal)
│   └─ PhaseCompleted(ReapExpired)
│
├─[discovery:bootstrap]  (if region has no sources)
│   └─ DiscoveryEvent::SourceDiscovered (seed sources)
│
▼
PhaseCompleted(ReapExpired)
│
└─[lifecycle:schedule]
    └─ PipelineEvent::ScheduleResolved { scheduled_data, actor_contexts, url_mappings }
    └─ SourcesScheduled { tension_count, response_count }
    │
    ▼
    SourcesScheduled
    │
    └─[scrape:tension]
        └─ ScrapeEvent::ContentFetched / ContentUnchanged / ContentFetchFailed (per URL)
        └─ SignalEvent::SignalsExtracted { batch } (per URL → triggers dedup sub-chain)
        └─ ScrapeEvent::LinkCollected (per discovered link)
        └─ ScrapeEvent::SocialPostsFetched (per social source)
        └─ PipelineEvent::ScrapeAccumulated { ... }
        └─ PhaseCompleted(TensionScrape)
        │
        ▼
        PhaseCompleted(TensionScrape)
        │
        ├─[discovery:link_promotion]
        │   └─ DiscoveryEvent::SourceDiscovered (promoted links)
        │   └─ DiscoveryEvent::LinksPromoted
        │
        └─[discovery:source_expansion]
            └─ DiscoveryEvent::SourceDiscovered (expansion sources)
            └─ DiscoveryEvent::ExpansionQueryCollected
            └─ DiscoveryEvent::SocialTopicCollected
            └─ PipelineEvent::SocialTopicsCollected
            └─ PhaseCompleted(SourceExpansion)
            │
            ▼
            PhaseCompleted(SourceExpansion)
            │
            └─[scrape:response]
                └─ (same pattern as scrape:tension)
                └─ PipelineEvent::SocialTopicsConsumed
                └─ PipelineEvent::ScrapeAccumulated
                └─ PhaseCompleted(ResponseScrape)
                │
                ▼
                PhaseCompleted(ResponseScrape)
                │
                ├─[discovery:link_promotion]
                │   └─ DiscoveryEvent::SourceDiscovered / LinksPromoted
                │
                ├─[enrichment:actor_extraction]
                │   └─ SystemEvent::PinsConsumed
                │   └─ SystemEvent::ActorIdentified (actor extraction)
                │   └─ EnrichmentRoleCompleted(ActorExtraction)
                │
                ├─[enrichment:diversity]
                │   └─ SignalDiversityComputed
                │   └─ EnrichmentRoleCompleted(Diversity)
                │
                ├─[enrichment:actor_stats]
                │   └─ ActorStatsComputed
                │   └─ EnrichmentRoleCompleted(ActorStats)
                │
                ├─[enrichment:actor_location]
                │   └─ SystemEvent::ActorLocationIdentified
                │   └─ EnrichmentRoleCompleted(ActorLocation)
                │
                └─[enrichment:phase_complete]
                    └─ PhaseCompleted(ActorEnrichment)  (when all 4 roles done)
                    │
                    ▼
                    PhaseCompleted(ActorEnrichment)
                    │
                    └─[enrichment:metrics]
                        └─ SystemEvent::SourceChanged (weight/cadence)
                        └─ SystemEvent::SourceScraped
                        └─ MetricsCompleted
                        │
                        ▼
                        MetricsCompleted
                        │
                        └─[expansion:signal_expansion]
                            └─ DiscoveryEvent::SourceDiscovered
                            └─ PipelineEvent::ExpansionAccumulated
                            └─ PhaseCompleted(SignalExpansion)
                            │
                            ▼
                            PhaseCompleted(SignalExpansion)
                            │
                            ├─[discovery:link_promotion]
                            │
                            └─[synthesis:trigger]
                                └─ SynthesisEvent::SynthesisTriggered
                                │
                                ├─[synthesis:similarity]
                                │   └─ SystemEvent::SimilarityEdgesRebuilt
                                │   └─ SynthesisRoleCompleted(Similarity)
                                │
                                ├─[synthesis:response_mapping]
                                │   └─ SystemEvent::ResponseLinked
                                │   └─ SynthesisRoleCompleted(ResponseMapping)
                                │
                                ├─[synthesis:tension_linker]
                                │   └─ SystemEvent::ConcernLinked
                                │   └─ SynthesisRoleCompleted(ConcernLinker)
                                │
                                ├─[synthesis:response_finder]
                                │   └─ WorldEvent::ResourceLinked
                                │   └─ DiscoveryEvent::SourceDiscovered
                                │   └─ SynthesisRoleCompleted(ResponseFinder)
                                │
                                ├─[synthesis:gathering_finder]
                                │   └─ WorldEvent::GatheringAnnounced
                                │   └─ DiscoveryEvent::SourceDiscovered
                                │   └─ SynthesisRoleCompleted(GatheringFinder)
                                │
                                └─[synthesis:investigation]
                                    └─ SystemEvent::ObservationCorroborated
                                    └─ SynthesisRoleCompleted(Investigation)
                                │
                                ▼ (all 6 roles completed)
                                [synthesis:phase_complete]
                                └─ PhaseCompleted(Synthesis)
                                │
                                ▼
                                PhaseCompleted(Synthesis)
                                │
                                ├─[synthesis:severity_inference]
                                │   └─ SystemEvent::SeverityClassified
                                │
                                ├─[situation_weaving:run]  (full engine only)
                                │   └─ SystemEvent::SituationIdentified / Changed / Promoted
                                │   └─ SystemEvent::CuriosityTriggered
                                │   └─ SystemEvent::SourcesBoostedForSituation
                                │   └─ PhaseCompleted(SituationWeaving)
                                │   │
                                │   ▼
                                │   PhaseCompleted(SituationWeaving)
                                │   │
                                │   └─[supervisor:run]
                                │       └─ SystemEvent::DuplicateConcernMerged
                                │       └─ SystemEvent::DuplicateActorsMerged
                                │       └─ PhaseCompleted(Supervisor)
                                │       │
                                │       ▼
                                │       PhaseCompleted(Supervisor)
                                │       │
                                │       └─[lifecycle:finalize]
                                │           └─ RunCompleted { stats }  ← END
                                │
                                └─[lifecycle:finalize]  (scrape engine only)
                                    └─ RunCompleted { stats }  ← END
```

## Signal Processing Sub-Chain

Within each scrape phase, extracted signals trigger a self-contained causal sub-chain that runs within the same settle loop:

```
SignalEvent::SignalsExtracted { url, batch }
│
└─[signals:dedup]
    │
    ├─ NewSignalAccepted { node_id, node_type, pending_node }
    │   │
    │   └─[signals:create]
    │       └─ WorldEvent::{GatheringAnnounced | ResourceOffered | HelpRequested | ...}
    │       └─ WorldEvent::CitationPublished
    │       └─ SystemEvent::SensitivityClassified
    │       └─ SignalCreated { node_id }
    │           │
    │           └─[signals:wire_edges]
    │               └─ WorldEvent::SignalLinkedToSource
    │               └─ WorldEvent::ResourceLinked (per resource tag)
    │               └─ SystemEvent::ActorIdentified + ActorLinkedToSignal
    │               └─ SystemEvent::SignalTagged (per tag)
    │               └─ WorldEvent::ActorLinkedToSource
    │
    ├─ CrossSourceMatchDetected { existing_id, similarity }
    │   │
    │   └─[signals:corroborate]
    │       └─ WorldEvent::CitationPublished
    │       └─ SystemEvent::ObservationCorroborated
    │       └─ SystemEvent::CorroborationScored
    │
    ├─ SameSourceReencountered { existing_id, similarity }
    │   │
    │   └─[signals:refresh]
    │       └─ WorldEvent::CitationPublished
    │       └─ SystemEvent::FreshnessConfirmed
    │
    └─ DedupCompleted { url }
    └─ UrlProcessed { url, signals_created, signals_deduplicated }
```

## Phase Sequencing

Phases are sequenced by `PhaseCompleted` events, not by explicit orchestration. Each handler declares which `PhaseCompleted` variant it reacts to:

| Phase | Triggered By | Handlers |
|-------|-------------|----------|
| ReapExpired | `EngineStarted` | `lifecycle:reap` |
| Schedule | `PhaseCompleted(ReapExpired)` | `lifecycle:schedule` |
| TensionScrape | `SourcesScheduled` | `scrape:tension` |
| SourceExpansion | `PhaseCompleted(TensionScrape)` | `discovery:source_expansion` |
| ResponseScrape | `PhaseCompleted(SourceExpansion)` | `scrape:response` |
| ActorEnrichment | `PhaseCompleted(ResponseScrape)` | `enrichment:actor_location`, `enrichment:post_scrape` |
| Metrics | `PhaseCompleted(ActorEnrichment)` | `enrichment:metrics` |
| SignalExpansion | `MetricsCompleted` | `expansion:signal_expansion` |
| Synthesis | `PhaseCompleted(SignalExpansion)` | `synthesis:trigger` → 6 parallel role handlers → `synthesis:phase_complete` |
| SituationWeaving | `PhaseCompleted(Synthesis)` | `situation_weaving:run` |
| Supervisor | `PhaseCompleted(SituationWeaving)` | `supervisor:run` |
| Finalize | `PhaseCompleted(Synthesis)` or `PhaseCompleted(Supervisor)` | `lifecycle:finalize` |

Link promotion (`discovery:link_promotion`) fires after `PhaseCompleted(TensionScrape)`, `PhaseCompleted(ResponseScrape)`, and `PhaseCompleted(SignalExpansion)`.
