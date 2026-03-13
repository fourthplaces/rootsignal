# Event Taxonomy

Scout uses a three-layer event taxonomy. Events are facts — they describe what happened, not commands to do something.

## Layer 1: World Events

Things that happened in reality. Intended to be public and portable (future `rootsignal-world` crate). Contains zero system-internal or sensitive information.

### Signal Creation

| Event | Description |
|-------|-------------|
| `GatheringAnnounced` | A time-bound community event was discovered |
| `ResourceOffered` | An available resource (food, shelter, tools) was discovered |
| `HelpRequested` | A community need or volunteer call was discovered |
| `AnnouncementShared` | An official notice or advisory was discovered |
| `ConcernRaised` | A systemic tension or conflict was discovered |
| `ConditionObserved` | An environmental or infrastructure condition was observed |

### Citations and Resources

| Event | Description |
|-------|-------------|
| `CitationPublished` | Source evidence linking a signal to a URL, content hash, and retrieval time |
| `ResourceLinked` | A resource (link, document) attached to a signal |
| `ResourceIdentified` | A resource entity was identified from content |

### Provenance and Lifecycle

| Event | Description |
|-------|-------------|
| `SignalLinkedToSource` | Signal attributed to a data source |
| `ActorLinkedToSource` | Actor (org/person) attributed to a data source |
| `SourceLinkDiscovered` | One source discovered via another |
| `GatheringCancelled` | A gathering was retracted or cancelled |
| `ResourceDepleted` | A resource is no longer available |
| `AnnouncementRetracted` | An announcement was withdrawn |
| `CitationRetracted` | A citation was invalidated |
| `DetailsChanged` | Signal details were updated from a new scrape |

## Layer 2: System Events

Decisions the system makes about world facts. Editorial judgments, classifications, and administrative actions.

### Classification

| Event | Description |
|-------|-------------|
| `SensitivityClassified` | PII/sensitivity level assigned (General / Elevated / Sensitive) |
| `ToneClassified` | Tone assessment assigned to a signal |
| `SeverityClassified` | Severity level assigned to a notice/tension |
| `UrgencyClassified` | Urgency level assigned to a signal |
| `CategoryClassified` | Signal category assigned (universal metadata on NodeMeta) |

### Signal Lifecycle

| Event | Description |
|-------|-------------|
| `FreshnessConfirmed` | Signal re-encountered at same source — still active |
| `ConfidenceScored` | Quality confidence score computed |
| `CorroborationScored` | Source diversity / corroboration score updated |
| `ObservationRejected` | Signal failed quality gate |
| `EntityExpired` | Signal past TTL — soft-deleted (`expired = true`) |
| `EntityPurged` | Signal hard-deleted (365-day absolute cutoff) |
| `DuplicateDetected` | Signal identified as duplicate of existing |
| `ImpliedQueriesExtracted` | Follow-up search queries extracted from signal content |

### Corrections

| Event | Description |
|-------|-------------|
| `GatheringCorrected` | Gathering details corrected from new evidence |
| `ResourceCorrected` | Resource details corrected |
| `HelpRequestCorrected` | Help request details corrected |
| `AnnouncementCorrected` | Announcement details corrected |
| `ConcernCorrected` | Concern details corrected |

Corrections are events, not mutations — they layer on top of the archival record.

### Actors

| Event | Description |
|-------|-------------|
| `ActorIdentified` | An organization or person was identified |
| `ActorLinkedToSignal` | Actor associated with a signal |
| `ActorLocationIdentified` | Actor's location triangulated from signal evidence |
| `DuplicateActorsMerged` | Duplicate actor nodes merged |
| `OrphanedActorsCleaned` | Actors with no remaining signals removed |

### Relationships

| Event | Description |
|-------|-------------|
| `ResponseLinked` | Resource/Gathering linked as responding to HelpRequest/Concern |
| `ConcernLinked` | Signal linked to an existing concern |
| `ObservationCorroborated` | Signal confirmed by additional source |

### Situations

| Event | Description |
|-------|-------------|
| `SituationIdentified` | New situation cluster created from related signals |
| `SituationChanged` | Situation metadata updated (title, summary, arc) |
| `SituationPromoted` | Situation elevated in prominence |
| `DispatchCreated` | Action dispatch generated for a situation |
| `CuriosityTriggered` | System flagged a gap worth investigating |
| `SourcesBoostedForSituation` | Source weights boosted for hot situations |

### Sources

| Event | Description |
|-------|-------------|
| `SourceRegistered` | New data source registered |
| `SourceChanged` | Source metadata updated (weight, cadence, URL) |
| `SourceDeactivated` | Source disabled due to poor signal yield |
| `SourceSystemChanged` | Source system-level attribute changed |
| `SourceScraped` | Source scrape completed with results |

### Tags and Quality

| Event | Description |
|-------|-------------|
| `SignalTagged` | Tag applied to a signal |
| `TagSuppressed` | Tag removed or suppressed |
| `TagsMerged` | Duplicate tags merged |
| `EmptyEntitiesCleaned` | Entities with no data cleaned up |
| `FakeCoordinatesNulled` | Invalid city-center coordinates removed |
| `OrphanedCitationsCleaned` | Citations with no parent signal removed |

### User Actions

| Event | Description |
|-------|-------------|
| `PinCreated` | User pinned a signal for attention |
| `PinsConsumed` | Consumed pins processed and cleared |
| `DemandReceived` | External demand/request received |
| `SubmissionReceived` | User-submitted signal received |

## Layer 3: Pipeline Events

Internal bookkeeping. Not part of the domain model — these drive run execution and aggregate state mutations.

### Lifecycle Events

| Event | Description |
|-------|-------------|
| `EngineStarted` | Entry point — triggers the entire pipeline |
| `PhaseStarted` | Phase transition marker (carries `PipelinePhase`) |
| `PhaseCompleted` | Phase completion marker — triggers next phase's handlers |
| `SourcesScheduled` | Sources loaded and partitioned for scraping |
| `MetricsCompleted` | Source metrics updated, triggers expansion |
| `RunCompleted` | Terminal event — carries final `ScoutStats` |

### Pipeline State Events

These exist solely to mutate `PipelineState` through the aggregate, replacing direct handler writes:

| Event | Description |
|-------|-------------|
| `ScheduleResolved` | Stash scheduled sources, actor contexts, URL mappings |
| `ScrapeAccumulated` | Accumulate scrape output: URL maps, signal counts, pub dates, links |
| `ExpansionAccumulated` | Accumulate expansion output: topics, stats |
| `SocialTopicsCollected` | Stash mid-run social topics for response scrape |
| `SocialTopicsConsumed` | Clear social topics after consumption |

### Domain-Internal Events

These drive the signal processing sub-chain and scrape telemetry. They are persisted but not projected to Neo4j.

**SignalEvent**: `SignalsExtracted`, `NewSignalAccepted`, `CrossSourceMatchDetected`, `SameSourceReencountered`, `DedupCompleted`, `SignalCreated`, `UrlProcessed`

**ScrapeEvent**: `ContentFetched`, `ContentUnchanged`, `ContentFetchFailed`, `SignalsExtracted`, `ExtractionFailed`, `SocialPostsFetched`, `FreshnessRecorded`, `LinkCollected`

**DiscoveryEvent**: `SourceDiscovered` (projectable), `LinksPromoted`, `ExpansionQueryCollected`, `SocialTopicCollected`

**EnrichmentEvent**: `EnrichmentRoleCompleted` (carries `EnrichmentRole` — one of: ActorExtraction, Diversity, ActorStats, ActorLocation)

**SynthesisEvent**: `SynthesisTriggered`, `SynthesisRoleCompleted` (carries `SynthesisRole` — one of: Similarity, ResponseMapping, ConcernLinker, ResponseFinder, GatheringFinder, Investigation)

## Persistence and Projection Matrix

| Event Category | Persisted (Postgres) | Projected (Neo4j) | Folded (Aggregate) |
|---------------|---------------------|-------------------|-------------------|
| WorldEvent | Yes | Yes | No |
| SystemEvent | Yes | Yes | No |
| LifecycleEvent | Yes | No | No |
| PipelineEvent | Yes | No | Yes |
| SignalEvent | Yes | No | Yes |
| ScrapeEvent | Yes | No | Yes |
| DiscoveryEvent | Yes | `SourceDiscovered` only | Yes |
| EnrichmentEvent | Yes | No | No |
| SynthesisEvent | Yes | No | Yes |
