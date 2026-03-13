# Refactor: World Event Taxonomy

## Philosophy

Events are facts about the world — not facts about the system observing it. Each event name should describe **what happened**, not **what the system did**. "HelpRequested" is a world fact (someone asked for help). "NeedDiscovered" leaks the system boundary (the system found a need). The event existed before we saw it.

The test for every event: **"Would this be true if our system didn't exist?"**

The test for World vs System placement: **"Does this relationship exist independent of our system, or is it an artifact of our processing?"**

No event implies authority. Everyone is equal in Root Signal. A neighbor on Nextdoor and a government agency both share announcements — neither carries inherent authority in the taxonomy.

### The world layer vs the system layer

The world layer is the corkboard — facts pinned to it. The system layer is the red string connecting the pins. A human could make the same judgments the system makes (identifying actors, linking related signals, scoring corroboration). Those judgments are real work, but they're not the world itself. A different human might connect the string differently.

The test for placement: **"Is this a fact about the world, or a judgment about the world?"**

---

## The Seven World Event Types

### 1. GatheringAnnounced
People are coming together at a time and place.

Community cleanups, protests, workshops, ceremonies, potlucks, town halls, restoration events, religious gatherings, mutual aid distributions, solidarity marches.

### 2. ResourceOffered
Something is being made available to the community.

Food shelves, legal clinics, warming shelters, tool libraries, free tutoring, community fridges, NGO deployments, shared kitchen space, bilingual navigation services. Neutral about power dynamics — covers tangible resources (food, shelter, rides) and intangible ones (expertise, space, time). Maps directly to the resource-matching vision where signals carry Requires/Offers edges.

### 3. HelpRequested
Someone needs something.

Mutual aid requests, GoFundMe campaigns, volunteer calls, disaster relief needs, organization staffing gaps, community garden needing supplies, food bank needing drivers.

### 4. AnnouncementShared
Information was shared with the community.

Policy changes, road closures, new programs, rate increases, schedule changes, election information, program updates. No authority implied — a city council resolution and a neighborhood Facebook post are both announcements.

### 5. ConcernRaised
Someone voiced a concern. Always a human act.

Community tensions, complaints about environmental harm, safety concerns, opposition to development, reports of enforcement activity, neighborhood quality of life issues. The human voice — distinct from measured conditions.

### 6. ConditionObserved
A state of the world was measured or recorded. Not a human opinion — a fact about reality.

Water quality readings, species counts, air quality data, infrastructure assessments, economic indicators, encampment counts, ecological surveys, citizen science observations.

### 7. IncidentReported
A discrete event occurred in the world. Not a persistent condition — something that happened at a point in time.

Fires, explosions, earthquakes, spills, collapses, accidents, closures. The distinction from ConditionObserved: states persist, incidents happen. "Dissolved oxygen is low" is a condition. "The plant exploded" is an incident. "The business closed on February 15" is an incident, not a condition.

---

## ConcernRaised + ConditionObserved: The Bidirectional Link

These two types are complementary world facts that can arrive in either order:

**Path A — Concern first:**
```
ConcernRaised("There's pollution in the river!")
  -> system investigates -> finds EPA water quality data
  -> ConditionObserved("Minnehaha Creek dissolved oxygen below safe threshold")
  -> linked, both now grounded
```

**Path B — Condition first:**
```
ConditionObserved("Monarch counts 40% below baseline in Dakota County")
  -> system asks: is anyone concerned? anyone responding?
  -> finds ConcernRaised + GatheringAnnounced (habitat restoration event)
  -> linked, the condition has context and response
```

This is the curiosity loop from tension-gravity working bidirectionally. ConditionObserved declares the state of reality. ConcernRaised declares that someone recognizes misalignment. The link between them is understanding.

The human/measurement split is critical:
- "Brown water coming out of taps" (resident report) -> **ConcernRaised**
- "Turbidity measured at 15 NTU" (city test) -> **ConditionObserved**
- Same reality, two different kinds of world facts

---

## Renames (WorldEvent enum)

| Current | New | Rationale |
|---|---|---|
| GatheringDiscovered | GatheringAnnounced | The gathering was announced — that's the world event |
| AidDiscovered | ResourceOffered | Someone made a resource available — neutral about power dynamics |
| NeedDiscovered | HelpRequested | Someone asked for help — that happened before we saw it |
| NoticeDiscovered | AnnouncementShared | Someone shared information — no authority implied |
| TensionDiscovered | ConcernRaised | Someone raised a concern — a human act |
| *(new)* | ConditionObserved | A state of the world was measured — gives ecological signal and citizen science a natural home |
| *(new)* | IncidentReported | A discrete event occurred — fires, closures, earthquakes. Distinct from ConditionObserved (states persist, incidents happen) |
| CitationRecorded | CitationPublished | A citation was published — "Recorded" leaks the system boundary |
| ResourceEdgeCreated | ResourceLinked | A real-world resource relationship exists |

### Field changes on AnnouncementShared (formerly NoticeDiscovered)

- **Remove** `source_authority` — antithetical to the project's principle that no one carries inherent authority
- **Remove** `severity` — a classification, not a world fact. Moves to SystemEvent as SeverityClassified.
- **Keep** `category`, `effective_date`

### Field changes on all signal types

- **Remove** `confidence: f32` — this is a system assessment, not a world fact. The existing `ConfidenceScored` system event handles this.
- **Remove** `extracted_at` — redundant with the Event envelope timestamp. The world event records when it was published (`published_at`), not when the system processed it.
- **Remove** `organizer` from GatheringAnnounced — redundant with Entity `role: "organizer"` in mentioned_entities.
- **Remove** `is_ongoing` from ResourceOffered — redundant with Schedule. If it has a schedule, it's ongoing.
- **Remove** `severity` from ConcernRaised and AnnouncementShared — a classification, not a world fact. Move to SystemEvent as `SeverityClassified { signal_id, severity }`.
- **Remove** `urgency` from AnnouncementShared and HelpRequested — same reasoning. Move to SystemEvent as `UrgencyClassified { signal_id, urgency }`.
- **Replace** `mentioned_actors: Vec<String>` and `author_actor: Option<String>` with `mentioned_entities: Vec<Entity>` — a unified entity reference system. The author is just an entity with `role: Some("author")`.
- **Replace** `location: Option<Location>` and `from_location: Option<Location>` with `locations: Vec<Location>` — multiple typed locations. A march has "start" and "end." A watershed concern has multiple points. A resource has "origin" and "destination." Scope is implicit in the geography — one point is block-level, five points across a watershed is regional.
- **Add** `extraction_id: Option<Uuid>` — groups events extracted from the same source. When a single newspaper article produces a GatheringAnnounced, a ConcernRaised, and a CitationPublished, they all share the same extraction_id. Makes the audit trail explicit and multi-event extraction traceable.
- **Add** `references: Vec<Reference>` — stated relationships to other world facts as described by the source. When a source says "this cleanup was organized because of the oil spill," that's a world fact. Different from system-inferred links (which are judgments in SystemEvent).
- **Promote** `schedule: Option<Schedule>` from GatheringAnnounced to the shared base. A food shelf open every Tuesday, a monthly town hall, a weekly water quality reading — all signal types can recur.
- **Remove** tone/prosody from world events — classifying emotional register or text delivery style is a judgment, not a world fact. Tone moves to SystemEvent as `ToneClassified { signal_id, tone: Tone }` alongside `SensitivityClassified` and `ConfidenceScored`. When voice processing arrives (e.g., Hume), measurable waveform data (pitch, rate, volume) would be world facts, but text register classification is not.

### Entity type

```rust
struct Entity {
    name: String,
    entity_type: EntityType,
    role: Option<String>,  // "author", "organizer", "subject", "location"
}

enum EntityType {
    Person,        // individual humans
    Organization,  // named, structured groups of people
    Group,         // unnamed or loosely defined collections of people
    Place,         // geographic — natural or built
    Thing,         // everything else — species, legislation, infrastructure, programs
}
```

Five types. Person, Organization, Group, Place, Thing. "Displaced families" = Group. "Long-time renters" = Group. "Unhoused residents" = Group. Different from Organization (which is named and structured). Disambiguation works — "Monarch" the butterfly (Thing) vs "Monarch" the building (Place) vs "Monarch" the organization (Organization).

Every entity reference becomes a potential node and edge in the knowledge graph. Two events sharing an entity with the same name and type is a concrete, explicit overlap — no semantic similarity needed for link discovery.

### Location type

```rust
struct Location {
    lat: f64,
    lng: f64,
    name: Option<String>,
    role: Option<String>,  // "venue", "origin", "destination", "affected_area", "epicenter"
}
```

Multiple locations per event. A gathering has a "venue." A march has "start" and "end." A watershed concern has multiple "affected_area" points. Replaces the single `location` + `from_location` fields.

### Reference type

```rust
struct Reference {
    description: String,          // "the recent oil spill on Minnehaha Creek"
    relationship: Relationship,   // typed, not freeform
}

enum Relationship {
    RespondsTo,    // "this cleanup was organized because of the spill"
    CausedBy,      // "the encampment appeared after the shelter closed"
    Updates,       // "new information about the same situation"
    Contradicts,   // "this data says the opposite"
    Supports,      // "this report backs up the claim" (absorbs BasedOn — distinction too subtle)
    Supersedes,    // "this replaces the previous announcement"
}
```

Stated relationships as described by the source — world facts, not system inferences. The relationship is an enum, not a freeform string, so the extraction pipeline picks from a fixed set and the relationship graph is queryable. The system can match reference descriptions to existing signals, but the reference itself is what the source said. This is the world-layer version of what ResponseLinked and TensionLinked were trying to capture, grounded in what the source stated rather than what the system inferred.

### Tone (SystemEvent, not WorldEvent)

Tone is a judgment — the system's classification of the emotional register. Moved to SystemEvent as `ToneClassified { signal_id, tone: Tone }`:

```rust
enum Tone {
    Urgent,
    Distressed,
    Fearful,
    Grieving,
    Angry,
    Defiant,
    Hopeful,
    Supportive,
    Celebratory,
    Analytical,
    Neutral,
}
```

Expanded to cover contested and crisis scenarios. Sits alongside `SensitivityClassified` and `ConfidenceScored` in the editorial layer. When voice processing arrives (e.g., Hume AI), measurable vocal features (pitch, rate, volume) would be world facts on the event; emotion predictions from those features would still be system judgments landing here.

### CitationPublished

No new fields beyond the existing shape. Media type (article, video, image, etc.) is inferable from the source URL at read time — `instagram.com/stories/...` is obviously a video, `epa.gov/reports/...` is a document. The audit process fetches the URL to verify anyway, so storing media type on the event duplicates what the URL already tells you.

### Shared base across all signal types

```
id, title, summary, source_url
published_at, extraction_id
locations: Vec<Location>
mentioned_entities: Vec<Entity>
references: Vec<Reference>
schedule: Option<Schedule>
```

### ConditionObserved fields

Uses the shared base. Structured measurement data lives in the summary. Keeps the shape consistent across all signal types.

### World-layer lifecycle events

New event types for when the world changes. These are world facts, not corrections — the original event remains true (it was announced), but a new fact supersedes it. The log preserves both.

- **GatheringCancelled** — the organizer cancelled the event. Carries `signal_id`, `reason`, `source_url`.
- **ResourceDepleted** — the resource ran out. Same pattern.
- **AnnouncementRetracted** — the announcement was withdrawn. Same pattern.
- **CitationRetracted** — a source artifact was retracted or withdrawn. Critical for epistemic accountability — when a paper is retracted, the system can walk the citation graph and flag dependent events.
- **DetailsChanged** — a world fact was updated by its source. "The organizer moved the gathering to Thursday." Carries `signal_id`, `summary` (what changed), `source_url`. The original event remains true (it was announced for Wednesday). This is a new fact that supersedes the detail. One generic event instead of type-specific update events.

---

## Events Moving from SystemEvent to WorldEvent

These pass the "is this a fact about the world?" test — an investigative journalist pinning threads on a corkboard would track all of these.

| Event | Rationale |
|---|---|
| ActorLinkedToSource | "This journalist writes for the Star Tribune" — real-world provenance |
| SignalLinkedToSource | "This article was published by the Star Tribune" — real-world provenance |
| SourceLinkDiscovered | "This blog is a subdomain of this media org" — real-world relationship |

---

## Events Moving from WorldEvent to SystemEvent

These are judgments about the world — a human could make the same calls, but a different human might disagree. They don't describe what happened; they describe what the system concluded.

| Event | Rationale |
|---|---|
| ObservationCorroborated | The system decided two sources are about the same thing — a judgment. The world fact is just that the citation exists (CitationPublished). Scoring already lives in CorroborationScored. |
| ActorIdentified | The system identified an actor from a source — editorial extraction, not a world event |
| ActorLinkedToSignal | The system decided this actor is involved with this signal — a judgment about participation |
| ActorLocationIdentified | The system determined where an actor is located — a judgment, not a world fact |
| ResponseLinked | The system decided a signal responds to a concern — a judgment about the relationship |
| TensionLinked | The system decided two concerns are related — a judgment about the relationship |

---

## SystemEvent Correction Renames

Correction events in SystemEvent reference the old signal type names and must stay in sync:

| Current | New |
|---|---|
| AidCorrected | ResourceCorrected |
| NeedCorrected | HelpRequestCorrected |
| NoticeCorrected | AnnouncementCorrected |
| TensionCorrected | ConcernCorrected |

---

## Pressure Testing Results

The taxonomy was tested against 35 detailed scenarios across 7 categories: domestic/community, ecological/environmental, crisis/disaster, political/contested, misinformation/propaganda, global/cross-cultural, and edge cases.

**Domestic/community:** Weekly community fridge (ResourceOffered + schedule), Nextdoor displacement complaint (ConcernRaised), city council bike lane vote (AnnouncementShared), GoFundMe for fire victims (HelpRequested + IncidentReported), block party (GatheringAnnounced), lead in school fountains investigative piece (multi-event: ConcernRaised + ConditionObserved + 3x CitationPublished with shared extraction_id), garden closing (IncidentReported), church tax prep (ResourceOffered + schedule).

**Ecological/environmental:** Monarch butterfly counts (ConditionObserved), PFAS across 3 counties (ConditionObserved + multiple locations), indigenous controlled burn (GatheringAnnounced + ConditionObserved), satellite deforestation imagery (ConditionObserved), fishing advisory (AnnouncementShared + ConditionObserved).

**Crisis/disaster:** Pakistan flooding (all 7 types fire across many sources), Turkey/Syria earthquake (cross-border with diaspora organizing — multiple locations handle this), chemical plant explosion (IncidentReported + AnnouncementShared evacuation + ConcernRaised + ResourceOffered medics), hurricane aftermath (multi-source documentation via extraction_id grouping).

**Political/contested:** Standing Rock (GatheringAnnounced + ConcernRaised + AnnouncementShared legal filings — opposing perspectives naturally handled as separate events), recurring protest march (GatheringAnnounced + schedule), armed militia (ConditionObserved presence + ConcernRaised from both sides), school board book ban (GatheringAnnounced + ConcernRaised from multiple perspectives), shelter defunding + encampment (AnnouncementShared + IncidentReported + ConcernRaised with causal Reference).

**Misinformation/propaganda:** Anti-vaccine viral post (structurally thin ConcernRaised — no ConditionObserved, no independent citations), coordinated fake news (SourceLinkDiscovered reveals shared parentage despite apparent independence), misquoted EPA report (ConditionObserved contradicts ConcernRaised — structural contradiction visible), 5G concern from YouTube (ungrounded concern pattern — single citation, no measurement data).

**Edge cases:** Dual offer+request post (ResourceOffered + HelpRequested with shared extraction_id), Juneteenth celebration+protest (GatheringAnnounced + optional ConcernRaised), person as both actor and subject (Entity role system handles cleanly), retracted paper (CitationRetracted propagates through citation graph), single tweet with 3 event types (multi-event extraction with shared extraction_id).

**Key architectural requirement:** The system must support multi-event extraction from a single source signal. Half the edge cases naturally produce 2-3 events of different types.

**Gaps identified and resolved during testing:**
- Endings/losses → absorbed into IncidentReported (a closure is an incident, not a condition)
- Unnamed populations ("displaced families") → Group entity type
- Incidents (fires, explosions) → IncidentReported (7th signal type, distinct from ConditionObserved)
- Citation retractions → CitationRetracted lifecycle event
- Modification tracking → DetailsChanged lifecycle event
- Tone classification → moved to SystemEvent (judgment, not world fact)
- Text register/prosody → dropped (still a judgment; voice waveform data is future work)
- Reference relationships → typed enum, not freeform strings

---

## Epistemic Accountability: Events as Self-Contained Facts

Every event stands on its own as a complete fact about the world. Each carries enough context to be understood independently — **what** it's about (topic, subject, entity references), **where** (location), **when** (published_at — when this fact entered the world), and **source** (where it came from).

The `published_at` date is the anchor to reality. Not when the system found it — when it was published. The evidence chain is a timeline of real-world artifacts, not a timeline of the system's processing.

### Links are discovered, not prescribed

Events don't need to know at creation time what they'll connect to. A `ConcernRaised` says "pollution in Minnehaha Creek." A `ConditionObserved` says "dissolved oxygen in Minnehaha Creek below threshold." A `CitationPublished` says "EPA report about Minnehaha Creek water quality." They all stand alone. But they're all *about* the same thing — and that's discoverable.

The arrows between events emerge from shared references — the same location, the same entity, the same topic, the same timeframe. The system finds the overlaps and makes the connections explicit. The links themselves are events too ("these two facts are about the same thing").

```
ConcernRaised("pollution in the river", published_at: 2026-02-15)
  ← a neighbor posted this on Nextdoor — stands alone

CitationPublished(url: epa.gov/..., published_at: 2026-01-20)
  ← the EPA report already existed — stands alone

ConditionObserved(dissolved oxygen low, published_at: 2026-01-20)
  ← extracted from the EPA report — stands alone

  ← system discovers all three are about Minnehaha Creek
  ← links created after the fact
```

### No "investigation started" events

Investigation is a system action — it's not a world fact. What IS a world fact is that a report exists, a measurement was taken, a citation was published. The system's investigation may be what *led us* to find these facts, but the facts themselves are what get recorded. This keeps the world event log grounded in reality, not in our processing.

### Community submissions work naturally

When the system opens for community submissions, each submission is a self-contained fact. It stands alone. Over time, the system (or other submissions) produce facts that overlap — same creek, same condition, same timeframe — and the web of connections grows organically. No one needs to know at submission time what their fact will connect to.

This is epistemic accountability: every claim in the system is traceable through a chain of independent, timestamped, sourced facts. The chain isn't constructed — it emerges from the overlap of what independent facts are *about*.

---

## Structural Integrity: How the Taxonomy Handles Misinformation

The taxonomy doesn't need a "misinformation" flag. Truth accumulates structure. Propaganda is structurally thin. The shape of the evidence is the judgment.

### The shape of a well-grounded fact

A real concern about water quality:

```
ConcernRaised("brown water from taps", Nextdoor, Feb 20)
ConditionObserved("turbidity 15 NTU", EPA, Feb 18)
CitationPublished(epa.gov/report, Feb 18)
CitationPublished(startribune.com/article, Feb 19)
CitationPublished(nextdoor.com/post, Feb 20)
```

Rich structure. Multiple independent sources. A human concern grounded in a measured condition. Citations from different publishers across different channels. High source diversity, high external ratio, corroboration across independent sources.

### The shape of propaganda

```
ConcernRaised("water poisoned by X", Facebook, Feb 20)
```

That's it. Look at what's absent:

- **No ConditionObserved** — no measurement, no data. A human voice with no grounding in measured reality.
- **No CitationPublished from independent sources** — no newspaper, no government report, no citizen science data.
- **Low source diversity** — maybe a few citations, but they all trace back to the same origin through SourceLinkDiscovered.
- **No provenance** — the actor behind it can't be traced to a known publisher or organization.

The claim stands alone. The log doesn't call it misinformation — it just doesn't have the supporting structure that truth tends to accumulate.

### Structural patterns

**Ungrounded concerns** — ConcernRaised with no corresponding ConditionObserved. The human voice exists, but reality hasn't been measured. Could be legitimate (nobody's tested the water yet), could be fabricated. Either way, the gap is visible.

**Measurements contradicting claims** — A ConcernRaised says "pollution is killing fish." A ConditionObserved says "dissolved oxygen is normal." The bidirectional link exists, but they point in opposite directions. The structure captures the contradiction without editorializing.

**Coordinated amplification** — Many CitationPublished events appearing in a narrow window, all pointing to the same claim, but SourceLinkDiscovered reveals they all trace back to the same parent. Looks like independent corroboration, but the provenance graph shows astroturfing.

**Concern-only actors** — An actor linked to many ConcernRaised but zero ResourceOffered, zero GatheringAnnounced. They raise alarm but never organize, never offer help. An agitation pattern visible in the actor's structural footprint.

**Claims posing as measurements** — A ConditionObserved with no CitationPublished. Someone presenting opinion as data, but there's no source artifact to verify. The evidentiary backbone is missing.

### The principle

Truth accumulates structure. Independent sources converge. Concerns get grounded in conditions. Citations come from diverse channels. Actors have rich footprints across multiple event types. Propaganda is structurally thin — single-source, ungrounded, narrow actor footprints. The event log doesn't need to judge. The shape of the evidence is visible to anyone who looks.

---

## What WorldEvent Becomes

After the refactor, WorldEvent contains only facts about the world:

- **7 signal types**: GatheringAnnounced, ResourceOffered, HelpRequested, AnnouncementShared, ConcernRaised, ConditionObserved, IncidentReported
- **CitationPublished**: A source artifact exists
- **ResourceLinked**: A real-world resource relationship exists
- **3 provenance events** (moved in from SystemEvent): ActorLinkedToSource, SignalLinkedToSource, SourceLinkDiscovered
- **5 lifecycle events**: GatheringCancelled, ResourceDepleted, AnnouncementRetracted, CitationRetracted, DetailsChanged

Everything else — actor identification, signal linking, corroboration, response/tension linking, tone classification — moves to SystemEvent as judgments.

---

## Extraction Pipeline Integration (mntogether curator)

The existing extraction library at `~/Developer/fourthplaces/mntogether` has infrastructure that maps directly to the new taxonomy. Rather than rebuilding, we adapt what exists.

### Reuse directly

**Evidence grounding** — The library already classifies every claim as DIRECT (quoted from source), INFERRED (reasonable inference), or ASSUMED (no evidence). This maps to the world/system split: DIRECT claims are world facts, INFERRED/ASSUMED are system judgments that should be flagged. In strict mode, ASSUMED claims are already filtered out.

**Conflict detection** — The library detects when sources disagree on the same topic, producing `Conflict { topic, claims }`. This is exactly the "measurements contradicting claims" structural pattern. Conflicts found during extraction can produce References with `relationship: Contradicts`.

**Partition strategy for multi-event extraction** — The partition system groups pages by distinct items and extracts each separately. This is the `extraction_id` grouping — a single article partitioned into a GatheringAnnounced + ConcernRaised + CitationPublished, all sharing the same extraction_id.

**Investigation loop for curiosity** — `while extraction.has_gaps()` drives follow-up queries to fill missing information. This is the curiosity loop — ConcernRaised with no grounding data triggers a search for ConditionObserved evidence. The gap system already classifies missing fields and generates ready-to-search queries.

**Source roles** — Primary/Supporting/Corroborating classification on citations. Feeds into the system-layer corroboration scoring and enrichment pipeline's source diversity metrics.

### Adapt

**Recall signals → event type hints** — The summarizer already extracts `calls_to_action` (→ HelpRequested or GatheringAnnounced), `offers` (→ ResourceOffered), `asks` (→ HelpRequested), and `entities` (→ mentioned_entities). These pre-extraction hints can seed the extraction pipeline's event type classification.

**Flat entity strings → typed Entity** — Entity extraction exists but produces flat strings. Upgrade to `Entity { name, entity_type: EntityType, role }` with the Person/Organization/Group/Place/Thing taxonomy.

**Source roles → CitationPublished metadata** — The source role (Primary/Supporting/Corroborating) is a system judgment about citation importance, not a property of the artifact. It feeds into SystemEvent-layer scoring, not the CitationPublished world event. The `media_type` on CitationPublished (Article/Video/Audio/etc.) is a separate world fact about the artifact itself.

### Build new

**Reference extraction** — The extraction pipeline needs a new stage to identify stated relationships in source text ("this cleanup was organized because of the spill") and produce typed References with Relationship enum values.

**Location extraction with roles** — Upgrade from single lat/lng to `locations: Vec<Location>` with typed roles (venue, origin, destination, affected_area, epicenter).

**Schedule extraction** — The summarizer may already surface schedule-like signals ("every Tuesday", "monthly"). Formalize this into the Schedule type on the shared base.

**Event type classification** — The extraction pipeline currently classifies into a flat post type. Upgrade to classify into the 7 world event types, potentially using the recall signals as hints.

---

## Implementation Scope

### Phase 1: New types and shared base
- Define Entity, EntityType, Location (updated), Reference, Relationship, Tone types
- Add IncidentReported variant to WorldEvent with shared base fields
- Add ConditionObserved variant to WorldEvent with shared base fields
- Add 5 lifecycle event variants: GatheringCancelled, ResourceDepleted, AnnouncementRetracted, CitationRetracted, DetailsChanged

### Phase 2: Rename existing WorldEvent variants
- Rename 6 existing signal variants + serde tags (GatheringDiscovered→GatheringAnnounced, etc.)
- Rename CitationRecorded→CitationPublished
- Rename ResourceEdgeCreated→ResourceLinked
- Update field on AnnouncementShared (remove source_authority, remove severity)
- Update shared base on all signal types: remove confidence, remove extracted_at, remove severity/urgency, remove organizer/is_ongoing (redundant with entities/schedule), replace mentioned_actors/author_actor with mentioned_entities, replace location/from_location with locations, add extraction_id, add references, promote schedule
- Update all match arms, event_type() strings, serde tags

### Phase 3: Move 6 events from WorldEvent to SystemEvent
- Move ObservationCorroborated, ActorIdentified, ActorLinkedToSignal, ActorLocationIdentified, ResponseLinked, TensionLinked
- Update all producers and consumers

### Phase 4: Move 3 events from SystemEvent to WorldEvent
- Move ActorLinkedToSource, SignalLinkedToSource, SourceLinkDiscovered
- Update all producers and consumers

### Phase 5: New SystemEvent variants
- Add ToneClassified { signal_id, tone: Tone } to SystemEvent
- Add SeverityClassified { signal_id, severity: Severity } to SystemEvent
- Add UrgencyClassified { signal_id, urgency: Urgency } to SystemEvent
- Rename correction variants: AidCorrected→ResourceCorrected, NeedCorrected→HelpRequestCorrected, NoticeCorrected→AnnouncementCorrected, TensionCorrected→ConcernCorrected
- Update corresponding correction type structs
- Remove Confidence variant from all correction enums (handled by ConfidenceScored)
- Remove SourceAuthority variant from NoticeCorrection/AnnouncementCorrection
- Remove Severity variant from correction enums (handled by SeverityClassified)

### Phase 6: Update downstream consumers
- Update reducer.rs Cypher queries (event type matching, new field names, new types)
- Update pipeline event handlers
- Update extraction pipeline to produce new field shapes (mentioned_entities, locations, references, extraction_id)
- Update any API endpoints that reference event types
- Add serde aliases for backwards compatibility with existing stored events
