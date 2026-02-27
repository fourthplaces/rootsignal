# rootsignal-world

The vocabulary of reality. An append-only event schema describing what actually
happened in the world — independent of any system's editorial decisions.

Root Signal is a civic intelligence system that surfaces local reality: what's
happening, who's responding, and how to plug in. This crate defines the shared
language for talking about the world Root Signal observes. It is the **golden
thread** — the portable, public, civic infrastructure layer that Root Signal
builds on top of, but that any system could consume with its own editorial logic.

## The Three-Layer Model

Root Signal's event architecture separates three concerns:

| Layer | Crate | What it captures |
|-------|-------|-----------------|
| **1. World Facts** | `rootsignal-world` | Things observed in reality |
| **2. System Decisions** | `rootsignal-common` | Things the system decided about its model |
| **3. Operational Telemetry** | `rootsignal-common` | Infrastructure plumbing |

The litmus test for Layer 1: could you read this event aloud to a community
member and have it mean something to them?

"A gathering was announced at Lake Harriet Bandshell" — yes. That's a world fact.
"We re-scored confidence to 0.82 based on corroboration count" — no. That's a
system decision.

## Modules

### `events` — the golden thread

13 `WorldEvent` variants describing world facts. Serialized as tagged JSON
(`#[serde(tag = "type")]`) and stored in Postgres via the `Eventlike` trait.

**Discovery** — five signal types observed in the wild:
- `GatheringDiscovered` — a community convening (meeting, rally, cleanup, vigil)
- `AidDiscovered` — help being offered (food bank, legal clinic, shelter)
- `NeedDiscovered` — something missing (volunteers needed, supplies short)
- `NoticeDiscovered` — information published (policy change, closure, alert)
- `TensionDiscovered` — friction or crisis (eviction wave, pollution, conflict)

**Corroboration & Citations:**
- `ObservationCorroborated` — a second source independently confirms something
- `CitationRecorded` — specific text at a specific URL says this thing

**Actors:**
- `ActorIdentified` — a person, org, government body, or coalition exists
- `ActorLinkedToEntity` — an actor is involved in a signal
- `ActorLocationIdentified` — an actor's location was identified

**Relationships:**
- `ResourceEdgeCreated` — a need requires/prefers a resource, or aid offers one
- `ResponseLinked` — aid responds to a tension
- `GravityLinked` — a signal is drawn toward a tension

### `types` — the shared vocabulary

Domain enums that describe the world without system opinion:

- `NodeType` — Gathering, Aid, Need, Notice, Tension, Citation
- `ActorType` — Organization, Individual, GovernmentBody, Coalition
- `ChannelType` — Press, Social, DirectAction, CommunityMedia
- `DiscoveryMethod` — how a source was found (Curated, GapAnalysis, HumanSubmission, ...)
- `SourceRole` — what a source tends to surface (Tension, Response, Mixed)
- `SocialPlatform` — Instagram, Facebook, Reddit, Twitter, TikTok, Bluesky
- `EdgeType` — SourcedFrom, RespondsTo, DrawnTo, Requires, Prefers, Offers, ...
- `Urgency`, `Severity` — Low through Critical
- `GeoPoint`, `GeoPrecision` — coordinates with precision level
- `haversine_km` — great-circle distance between two points

### `values` — structured facts

- `Location` — where something is (point, name, address)
- `Schedule` — when something happens (starts_at, ends_at, rrule, timezone)

### `eventlike` — the storage trait

```rust
pub trait Eventlike: Debug + Send + Sync {
    fn event_type(&self) -> &'static str;
    fn to_payload(&self) -> serde_json::Value;
}
```

Implemented by `WorldEvent` (this crate), `SystemDecision`, and `TelemetryEvent`
(both in `rootsignal-common`). Enables generic event storage — any type that
implements `Eventlike` can be appended to the EventStore.

## Design Principles

**No Root Signal dependencies.** This crate depends only on serde, schemars,
uuid, and chrono. The compiler enforces the boundary — world vocabulary cannot
accidentally import system concepts.

**Public and portable.** Contains zero sensitive information. Sensitivity is a
gate at the ingestion boundary, not a label that travels with events. PII
scrubbing happens before events enter the golden thread.

**Append-only.** No fact disappears. Expiry is a soft-delete (Layer 2 sets
`expired = true`). Re-discovery clears the flag. The thread remembers everything.

**Slow to change.** Layer 1 is the archival record. It should evolve carefully
and with intention. Layer 2 can evolve rapidly without touching the world schema.

## What Does NOT Belong Here

- `SensitivityLevel` — Root Signal's privacy policy, not world vocabulary
- Confidence re-scoring — system opinion about corroboration strength
- Corrections — editorial judgments that the original observation was wrong
- Situations — clustering signals into narratives is an editorial decision
- Duplicate detection — system-computed similarity
- Implied queries — the system's expansion curiosity, not a world fact
- Sources — Root Signal's registry concept (source registration, changes, deactivation)
- Pins, demands, submissions — actions in Root Signal's app
- Actor-source links — links actors to system entities
- `SituationArc`, `Clarity`, `DispatchType` — system's clustering model concepts

The test: if a different system with different editorial logic would make a
different decision about it, it belongs in Layer 2, not here.
