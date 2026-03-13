# Golden Thread Event Audit — Brainstorm

## The Three-Layer Model

Auditing every event in `rootsignal-common/src/events.rs` against the Golden Thread spec revealed that "golden thread vs operational telemetry" is too coarse. There are actually three distinct layers:

### Layer 1: World Facts (the golden thread)
Things that happened in reality, independent of our system existing. A gathering was announced. A need was expressed. An actor exists. A source publishes information. A community member submitted something.

The test: could you read this event aloud to a community member and have it mean something to them?

### Layer 2: System Decisions (the editorial layer)
Things our system decided or did *in response to* world facts. We rejected an observation. We detected a duplicate. We re-scored confidence. We confirmed freshness. We corrected a title. We merged two actors. We expired an entity. We clustered signals into a situation.

Every one of these is the system talking about itself: "we rejected," "we detected," "we scored," "we merged," "we expired." These aren't things that happened in the world — they're things the system did to its *model* of the world.

### Layer 3: Operational Telemetry (the system log)
Infrastructure plumbing with no world-fact content. We scraped a URL. We spent N tokens. We cleaned up orphans. We consumed implied queries.

---

## Crate Architecture: `rootsignal-world`

The key insight: world facts are the valuable, portable, civic infrastructure layer. Root Signal is just one system that interprets them. A different system could consume the same world events with a different editorial layer and produce a different graph.

The world crate describes reality — the vocabulary for talking about the world. Events are one part of that vocabulary, but so are locations, schedules, actor types. They're all world concepts, not Root Signal concepts.

**The world crate is intended to be public.** It should contain exactly zero sensitive information. Sensitivity is a gate that prevents sensitive content from entering the thread, not a label that travels with events. PII scrubbing happens at the ingestion boundary, before events enter the golden thread.

### The separation

```
rootsignal-world/       ← NEW CRATE: the vocabulary of reality (public, portable)
  src/
    lib.rs              ← re-exports
    types.rs            ← Location, Schedule, GeoPoint, ActorType, Severity, etc.
    events.rs           ← WorldEvent enum
    events/
      discovery.rs      ← GatheringDiscovered, AidDiscovered, NeedDiscovered, etc.
      sources.rs        ← SourceRegistered, SourceChanged, SourceDeactivated, etc.
      actors.rs         ← ActorIdentified, ActorLinkedToEntity, etc.
      citations.rs      ← CitationRecorded
      community.rs      ← PinCreated, DemandReceived, SubmissionReceived

rootsignal-common/     ← EXISTING: Root Signal-specific system decisions + telemetry
  src/
    events.rs           ← SystemDecision enum (corrections, scoring, situations, etc.)
    telemetry.rs        ← TelemetryEvent enum (scrape stats, budget, housekeeping)
    safety.rs           ← SensitivityLevel + PII scrubbing (system policy, NOT world vocab)
```

Consuming code imports world types and world events separately:

```rust
use rootsignal_world::types::{Location, Schedule, NodeType, ActorType};
use rootsignal_world::events::WorldEvent;
```

### Why a separate crate, not just a separate module

1. **Dependency direction enforces purity.** `rootsignal-world` has no dependency on the rest of `rootsignal-*`. It can't accidentally import Root Signal system concepts. The compiler enforces the boundary.

2. **Portability.** Another civic tech project could depend on `rootsignal-world` without pulling in Root Signal's editorial machinery. The world types and world events become a shared protocol.

3. **Versioning independence.** Root Signal's editorial logic (Layer 2) can evolve rapidly — new scoring algorithms, new clustering approaches, new lint rules. The world schema (Layer 1) should evolve slowly and carefully, because it's the archival record. Separate crates, separate semver.

4. **The golden thread IS the world crate's events.** The append-only log that has civic value, that you'd publish or fork, is precisely the events defined in `rootsignal_world::events`. Nothing else.

5. **Room to grow.** The world crate isn't just events — it's the shared vocabulary. Future world concepts (regions, taxonomies, relationship types) have a natural home alongside `Location` and `Schedule`, without touching the event enum.

### What goes in `rootsignal-world`

**Types** (`types.rs`) — the vocabulary of reality:
- `Location`, `Schedule`, `GeoPoint`, `GeoPrecision` — where/when things happen
- `NodeType` — the five signal types (Gathering, Aid, Need, Notice, Tension)
- `ActorType` — what kind of actor (person, org, group)
- `ChannelType` — how information was published
- `DiscoveryMethod` — how a source was found
- `SourceRole` — what role a source plays
- `Severity`, `Urgency` — domain-level assessments
- `TagFact` — a tag with its weight

**Not in world crate:**
- `SensitivityLevel` — this is Root Signal policy (see "Sensitivity resolved" below)
- `SourceChange` — split needed (see "SourceChange split" below)

**Events** (`events.rs`) — facts about what happened in the world:
- The `WorldEvent` enum with all Layer 1 variants
- Serialization to/from JSON (serde)
- `event_type() -> &'static str` for each variant
- No graph concepts. No Neo4j. No reducers. No Root Signal.

### What stays in `rootsignal-common`

The existing crate keeps:
- `SystemDecision` enum — Root Signal's editorial interpretations of world facts
- `TelemetryEvent` enum — operational plumbing
- `SensitivityLevel` + PII scrubbing — Root Signal's privacy policy
- Correction enums (`GatheringCorrection`, etc.) — these are system opinions about world facts
- Situation types (`SituationArc`, `DispatchType`) — Root Signal's clustering model
- System-specific `SourceChange` variants (`QualityPenalty`, `GapContext`)

### The dependency graph

```
rootsignal-world  ←──  rootsignal-common  ←──  rootsignal-scout
                                            ←──  rootsignal-graph
                                            ←──  rootsignal-events
                                            ←──  rootsignal-api (ingestion boundary)
```

`rootsignal-common` depends on `rootsignal-world`. It re-exports world types so existing code doesn't break immediately. The system decision and telemetry events reference world types (e.g., `EntityExpired` references `NodeType` from `rootsignal_world::types`).

### External append model

External systems don't touch Postgres directly. The `rootsignal-world` crate is the schema; the Root Signal API is the ingestion boundary:

```
External agent
  → imports rootsignal-world
  → constructs WorldEvent
  → submits via Root Signal API
  → API validates (lint gate, PII scrubbing)
  → API assigns sequence number
  → API appends to events table
```

The system handles sequencing, gap-free reads, and all Postgres coordination. External agents only need to know the `WorldEvent` schema. This preserves the "permissionless append with verified claims" principle — the gate is verification, not access.

---

## Stress Test Findings

Four research agents analyzed the codebase against this architecture. Here's what survived and what broke.

### Resolved: SensitivityLevel is policy, not world vocabulary

`SensitivityLevel` encodes Root Signal's privacy policy — it has a `fuzz_radius()` method that reduces coordinate precision based on content sensitivity. That's a system opinion about how to handle data, not a world fact.

**Resolution:** `SensitivityLevel` stays in `rootsignal-common/safety.rs`. It does NOT move to the world crate. Discovery events in the world crate do NOT carry a sensitivity field.

The world crate is public. It should contain exactly zero sensitive information. Sensitivity classification is a gate at the ingestion boundary — sensitive content is either scrubbed before entering the golden thread, or it never enters at all. PII detection and scrubbing happen in `rootsignal-common`, not in the world crate.

### Resolved: Confidence has two meanings

Discovery events carry `confidence: f32`. This is the observer's stated confidence at discovery time — the LLM extraction saying "I'm 75% sure this gathering is real." A human would do this too. It's like a journalist writing "unconfirmed reports suggest..." That **is** a world fact: the observer's assessment of their own observation.

`ConfidenceScored` (Layer 2) is different — it's the system re-computing confidence from corroboration count, source weight, and freshness. That's a system opinion that overrides the original assessment.

**Resolution:** `confidence: f32` stays on discovery events in the world crate as the observer's initial assessment. `ConfidenceScored` stays in Layer 2 as the system's re-scoring.

### Resolved: ObservationCorroborated needs splitting

Currently carries `new_corroboration_count: u32` and `similarity: f64` — both system-computed values. The world fact is "source B also mentions this gathering." The count and similarity are system opinions.

**Resolution:** Split into two events:
- **Layer 1 (world crate):** `ObservationCorroborated { entity_id, node_type, new_source_url, summary }` — the world fact: another source confirms this
- **Layer 2 (rootsignal-common):** `CorroborationScored { entity_id, similarity, new_corroboration_count }` — the system's assessment of the corroboration

### Resolved: implied_queries is a system artifact

Every `*Discovered` event carries `implied_queries: Vec<String>`. These are Root Signal's expansion logic — "what questions does this observation suggest?" That's the system's curiosity, not a world fact.

**Resolution:** Strip `implied_queries` from world events. The existing `ExpansionQueryCollected` event (Layer 1) already captures "this observation implied this question." If needed, a Layer 2 event can capture the system's expansion decisions.

### Resolved: SourceChange needs splitting

`SourceChange::QualityPenalty` and `SourceChange::GapContext` are Root Signal internal metrics. World-appropriate variants are Weight, Url, Role, Active.

**Resolution:** Split the enum:
- **World crate:** `SourceChange` with `Weight`, `Url`, `Role`, `Active` variants
- **rootsignal-common:** `SystemSourceChange` with `QualityPenalty`, `GapContext` variants

### Resolved: discovery_depth is a system artifact

`ActorIdentified` carries `discovery_depth: Option<u32>` — how deep in Root Signal's expansion graph this actor was found. That's a system metric, not a world fact.

**Resolution:** Strip from world event. If needed, track in a Layer 2 event.

### Resolved: gap_context on SourceRegistered is a system artifact

`SourceRegistered` carries `gap_context: Option<String>` — Root Signal's gap analysis output.

**Resolution:** Strip from world event. The `discovery_method` field already captures provenance.

### CRITICAL: EntityExpired breaks layer-separated replay

The most concrete failure found:

```
Seq 10: GatheringDiscovered {id=X}     ← Layer 1, node created
Seq 20: EntityExpired {id=X}           ← Layer 2, node DELETED (DETACH DELETE)
Seq 30: GatheringDiscovered {id=X}     ← Layer 1, node recreated
```

**Interleaved replay:** Node exists (rediscovered after expiry).
**Layer-separated replay:** All Layer 1 runs first — seq 10 creates, seq 30 is a MERGE no-op. Then Layer 2 — seq 20 deletes. Node gone forever.

**Resolution:** EntityExpired becomes a **soft-delete**: `SET n.expired = true, n.expired_at = datetime($ts), n.expired_reason = $reason` instead of `DETACH DELETE`. Layer 1 re-discovery clears the expired flag via MERGE ON MATCH. This is also better for the golden thread principle — "no fact disappears." The thread remembers everything, including that something expired and was later rediscovered.

`EntityPurged` can remain a hard delete for truly garbage data, but it should be rare and deliberate.

### Not broken (confirmed safe)

| Scenario | Result |
|---|---|
| Correction ordering (Layer 2 after Layer 1) | Identical graphs — MATCH finds nodes created by Layer 1 |
| Multiple corrections to same entity | Identical — applied in sequence order within Layer 2 |
| Actor merge + linkage | Identical — MERGE is idempotent, merge repoints before deleting |
| Situation creation referencing entities | Works — Story nodes MERGE by situation_id, don't MATCH signals |
| Enrichment pass timing | Safe — runs after all projection, deterministic from graph state |

### Codebase refresh (post event-sourcing migration)

All signal writes now go through EventSourcedStore → event append → reducer projection. The GraphStore bypass has been removed. This is exactly what the three-layer model needs — every mutation is an event.

Three relationship events were missing from the original audit:
- `ResourceEdgeCreated` — "this need requires this resource" → Layer 1 (world relationship)
- `ResponseLinked` — "this aid responds to this tension" → Layer 1 (world relationship)
- `GravityLinked` — "this signal is drawn to this tension" → Layer 1 (world relationship)

**One event-sourcing gap found:** `TensionLinker` calls `GraphStore.create_response_edge()` directly instead of `store.create_drawn_to_edge()`, bypassing event emission for gravity links during tension creation. This is a bug to fix independently of the crate split.

### Not broken: Bootstrap self-containment

World events include `SourceRegistered` (Layer 1). Bootstrap emits `SourceRegistered` events into the golden thread. Someone replaying the golden thread gets all sources. The `BootstrapCompleted` telemetry event (Layer 3) is just an operational marker — it doesn't carry source data.

---

## Complete Event Classification (updated after stress test)

### Layer 1: World Facts → `rootsignal-world` crate

These describe things that happened in reality. No system-computed fields.

| Event | Fields to strip | Why it's a world fact |
|---|---|---|
| **Discovery events** | | |
| `GatheringDiscovered` | `sensitivity`, `implied_queries` | Someone is convening — the world said so |
| `AidDiscovered` | `sensitivity`, `implied_queries` | Help is being offered — the world said so |
| `NeedDiscovered` | `sensitivity`, `implied_queries` | Something is missing — the world said so |
| `NoticeDiscovered` | `sensitivity`, `implied_queries` | Something happened — the world said so |
| `TensionDiscovered` | `sensitivity`, `implied_queries` | Friction exists — the world said so |
| **Corroboration** | | |
| `ObservationCorroborated` | `similarity`, `new_corroboration_count` | A second source independently confirmed something |
| **Citations** | | |
| `CitationRecorded` | (none) | This specific text at this URL says this thing |
| **Sources** | | |
| `SourceRegistered` | `gap_context` | This information source exists in the world |
| `SourceChanged` | (split: world variants only) | Something about this source changed |
| `SourceDeactivated` | (none) | This source is no longer being monitored |
| `SourceLinkDiscovered` | (none) | This source is a child of that source |
| **Actors** | | |
| `ActorIdentified` | `discovery_depth` | This actor exists in the world |
| `ActorLinkedToEntity` | (none) | This actor is involved in this thing |
| `ActorLinkedToSource` | (none) | This actor publishes/runs this source |
| `ActorLocationIdentified` | (none) | This actor is located here |
| **Community input** | | |
| `PinCreated` | (none) | A human pinned a location as important |
| `DemandReceived` | (none) | A human asked for information about an area |
| `SubmissionReceived` | (none) | A human submitted a URL as a source |
| **Relationship edges** | | |
| `ResourceEdgeCreated` | (none) | This need requires / prefers / is offered this resource |
| `ResponseLinked` | (none) | This aid/gathering responds to this tension |
| `GravityLinked` | (none) | This signal is drawn to this tension |
| **Expansion provenance** | | |
| `ExpansionQueryCollected` | (none) | An observation implied a question worth investigating |

### Layer 2: System Decisions → `rootsignal-common` (SystemDecision enum)

These describe things Root Signal decided about its model of the world.

| Event | What the system decided |
|---|---|
| **Signal lifecycle decisions** | |
| `FreshnessConfirmed` | We checked and decided this signal is still current |
| `ConfidenceScored` | We re-computed confidence from corroborations and source weight |
| `CorroborationScored` | We computed similarity and updated corroboration count (NEW — split from ObservationCorroborated) |
| `ObservationRejected` | We examined a candidate and decided to exclude it |
| `EntityExpired` | We decided this signal is no longer current (soft-delete) |
| `EntityPurged` | We hard-removed this from the graph (rare, deliberate) |
| `DuplicateDetected` | We computed that two signals refer to the same thing |
| `ExtractionDroppedNoDate` | We dropped this because it lacked a date |
| **Sensitivity classification** | |
| `SensitivityClassified` | We classified this entity's sensitivity level (NEW — moved from discovery events) |
| **Correction decisions** | |
| `GatheringCorrected` | We decided a gathering's details were wrong and fixed them |
| `AidCorrected` | We decided an aid's details were wrong and fixed them |
| `NeedCorrected` | We decided a need's details were wrong and fixed them |
| `NoticeCorrected` | We decided a notice's details were wrong and fixed them |
| `TensionCorrected` | We decided a tension's details were wrong and fixed them |
| **Actor decisions** | |
| `DuplicateActorsMerged` | We decided these two actor records are the same entity |
| **Situation decisions** | |
| `SituationIdentified` | We clustered signals into a named situation |
| `SituationChanged` | We updated the situation's properties |
| `SituationPromoted` | We decided this situation should be surfaced |
| `DispatchCreated` | We produced a structured summary for output |
| **Tag decisions** | |
| `TagSuppressed` | We decided to hide this tag |
| `TagsMerged` | We decided these two tags are the same concept |
| **Human review decisions** | |
| `ReviewVerdictReached` | A human reviewed a signal and made a determination |

### Layer 3: Operational Telemetry → `rootsignal-common` (TelemetryEvent enum)

(Unchanged from previous version — scrape stats, agent telemetry, budget, housekeeping.)

### Events to Remove (redundant or derivable)

| Event | Why remove |
|---|---|
| `LintCorrectionApplied` | Redundant with typed `*Corrected` events — lint should emit those instead |
| `LintRejectionIssued` | Redundant with `ObservationRejected` — lint should emit that instead |
| `LintBatchCompleted` | Summary stat, derivable from individual lint events |
| `TagsAggregated` | Computed roll-up, derivable from entity content |
| `SourceScrapeRecorded` | Derivable from counting discovery events per source |
| `SourceRemoved` | Redundant with `SourceDeactivated` + reason |
| `SignalLinkedToSource` | Derivable from `source_url` in discovery event payloads |
| `ExpansionSourceCreated` | Fold into `SourceRegistered` with `discovery_method: Expansion` |

---

## Pressure Testing: Can We Do This Seamlessly?

### The store is already polymorphic — no schema change needed

The `events` table stores `event_type` (text) and `payload` (JSONB). It doesn't know or care about the Rust enum. Two crates serializing to the same table works out of the box. The store just needs to accept both `WorldEvent` and `SystemDecision` types, which it already does implicitly — it takes `serde_json::Value`.

A `stream` column (`'world'`, `'decision'`, `'telemetry'`) could be added to the table for filtered replay, but it's optional — you can derive stream membership from the `event_type` string.

### Events are already emitted independently — no bundling friction

Almost every event is its own `store.method().await?` call. The tightest coupling is in `store_signals()` where a corroboration is followed by a citation:

```rust
self.store.corroborate(...).await?;      // WorldEvent: ObservationCorroborated
self.store.create_evidence(...).await?;  // WorldEvent: CitationRecorded
```

(Note: with the ObservationCorroborated split, the corroboration world fact and the system scoring are now separate events, but both emitted in sequence. Still no bundling friction.)

### No circular dependencies between layers

Layer 1 events (discoveries, sources, actors, citations) use `MERGE` in the reducer — create-if-not-exists. They never reference Layer 2 state. They're self-sufficient.

Layer 2 events do `MATCH` on nodes that Layer 1 created. This is correct and expected — it's the natural ordering: world facts create nodes, system decisions annotate them. The crate boundary makes this explicit.

### The reducer composes cleanly

The current reducer is one big `match`. Under the crate split, it becomes:

```rust
pub async fn project(&self, event: &StoredEvent) -> Result<ApplyResult> {
    // Try world event first (from rootsignal-world crate)
    if let Ok(world_event) = WorldEvent::from_payload(&event.payload) {
        return self.apply_world_event(&world_event, event.seq).await;
    }
    // Then system decision (from rootsignal-common)
    if let Ok(decision) = SystemDecision::from_payload(&event.payload) {
        return self.apply_decision(&decision, event.seq).await;
    }
    // Then telemetry (no-op for the graph)
    if let Ok(_telemetry) = TelemetryEvent::from_payload(&event.payload) {
        return Ok(ApplyResult::NoOp);
    }
    warn!(seq = event.seq, "Unknown event type");
    Ok(ApplyResult::DeserializeError("unknown event type".into()))
}
```

The try-deserialize pattern works because `serde(tag = "type")` on each enum means only matching variants will deserialize successfully. No routing map needed — serde handles the dispatch.

### Ergonomic impact on call sites

The scout pipeline currently does:
```rust
use rootsignal_common::events::Event;

let event = Event::GatheringDiscovered { ... };
self.append_and_project(&event, None).await?;
```

After the split:
```rust
use rootsignal_world::events::WorldEvent;
use rootsignal_world::types::{Location, Schedule};

let event = WorldEvent::GatheringDiscovered { ... };
self.append_and_project(&event, None).await?;
```

The `append_and_project` method becomes generic over a trait:

```rust
pub trait Eventlike: Serialize {
    fn event_type(&self) -> &'static str;
    fn to_payload(&self) -> serde_json::Value;
}

impl Eventlike for WorldEvent { ... }      // from rootsignal-world
impl Eventlike for SystemDecision { ... }  // from rootsignal-common
impl Eventlike for TelemetryEvent { ... }  // from rootsignal-common

async fn append_and_project(&self, event: &impl Eventlike, actor: Option<&str>) -> Result<()>
```

Call sites change from `Event::` to `WorldEvent::` or `SystemDecision::` — a mechanical find-and-replace per event variant. Type imports change from `rootsignal_common::types::` to `rootsignal_world::types::`. No logic changes.

---

## Borderline Calls and Rationale

### Corrections in Layer 2, not Layer 1

The system decided a field was wrong and changed it. That's an editorial judgment. The world didn't correct itself — our system corrected its model. A different system with different lint rules might not make the same correction.

If you replay Layer 1 alone, you get the world as it was reported — including errors. If you replay Layer 1 + Layer 2, you get the world as editorially cleaned. Corrections do NOT retroactively fix the archival record. They layer on top.

### Situations in Layer 2, not Layer 1

Situations (clustering signals into narratives) are clearly system decisions. No one in the world said "this is a situation." The system looked at a collection of signals and decided they cohere. Different clustering logic would produce different situations from the same world facts.

Note: `SituationIdentified` currently carries derived metrics (`temperature`, `arc`, `structured_state`). These are computed values and arguably shouldn't be on the event at all if the reducer can derive them. But since situations are already Layer 2, this is consistent — Layer 2 events carry system opinions.

### ReviewVerdictReached in Layer 2

Even though a human made this decision, it's a decision *about the model*, not a fact about the world. "A reviewer decided this signal is trustworthy" is editorial judgment. It belongs in the editorial layer alongside the system's own decisions.

### ExpansionSourceCreated — fold into SourceRegistered

`ExpansionSourceCreated` is just a `SourceRegistered` with extra provenance (which query led to it). The `discovery_method` field on `SourceRegistered` already supports this. Fold it in rather than maintaining a separate event.

### External agents cannot create situations

The open-append model says external agents write verified world facts. "I found a gathering at this URL on this date" is verifiable. "I clustered 47 signals into a situation about housing affordability" is not — it's an editorial opinion. Situations are Layer 2 and can only be created by the system's editorial pipeline. This is consistent with situations NOT being in the world crate.

---

## Migration Path

### Phase 1: Create `rootsignal-world` crate
- Create `modules/rootsignal-world/` with `types.rs` and `events.rs`
- Move world-vocabulary types from `rootsignal-common/types.rs` → `rootsignal_world::types`
- Define `WorldEvent` enum with Layer 1 variants in `rootsignal_world::events`
- Strip system artifacts from world events: `sensitivity`, `implied_queries`, `discovery_depth`, `gap_context`, `similarity`, `new_corroboration_count`
- Split `SourceChange` into world and system variants
- Split `ObservationCorroborated` into world fact + `CorroborationScored` system decision
- Define `Eventlike` trait in `rootsignal-world` (both crates implement it)
- `rootsignal-common` depends on `rootsignal-world`, re-exports world types for compatibility
- All existing code continues to compile via re-exports — no downstream changes yet

### Phase 2: Split `rootsignal-common/events.rs`
- Extract `SystemDecision` enum with Layer 2 variants (including new `CorroborationScored`, `SensitivityClassified`)
- Extract `TelemetryEvent` enum with Layer 3 variants
- Remove the 8 redundant events
- Convert `EntityExpired` from hard-delete to soft-delete
- Old `Event` enum becomes a re-export wrapper or is removed

### Phase 3: Update consumers
- Reducer: split into `apply_world_event` + `apply_decision` methods
- EventSourcedStore: `append_and_project` becomes generic over `Eventlike`
- Scout call sites: `Event::` → `WorldEvent::` or `SystemDecision::`
- Type imports: `rootsignal_common::types::` → `rootsignal_world::types::`
- Ingestion pipeline: sensitivity classification emitted as separate Layer 2 event after world event
- Ingestion pipeline: PII scrubbing at boundary before world event is appended
- Add optional `stream` column to events table for filtered replay

### Phase 4: Replay verification
- Rebuild graph from world events only → base graph (raw observations)
- Rebuild from world events + system decisions → full graph (curated view)
- Compare full graph against current graph → verify equivalence
- Verify soft-delete EntityExpired + re-discovery produces same graph in both interleaved and layer-separated replay
