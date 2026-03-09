//! Three-layer event wrapper — dispatches to WorldEvent, SystemEvent, TelemetryEvent.
//!
//! The wrapper uses `#[serde(untagged)]` so deserialization tries each inner enum
//! in order (world → system → telemetry). Each inner enum uses `#[serde(tag = "type")]`
//! with distinct type tags, so exactly one will match.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Re-exports — value types from rootsignal-world
// ---------------------------------------------------------------------------

pub use rootsignal_world::values::{Location, Schedule};

// ---------------------------------------------------------------------------
// Re-exports — the three event layers
// ---------------------------------------------------------------------------

pub use crate::system_events::SystemEvent;
pub use crate::telemetry_events::TelemetryEvent;
pub use rootsignal_world::events::WorldEvent;

// ---------------------------------------------------------------------------
// Nested change enums — typed field mutations
// ---------------------------------------------------------------------------

use chrono::{DateTime, Utc};

use crate::safety::SensitivityLevel;
use crate::types::{Severity, SituationArc, Urgency};

/// System-layer source changes — editorial decisions about a source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum SystemSourceChange {
    QualityPenalty {
        old: f64,
        new: f64,
    },
    GapContext {
        old: Option<String>,
        new: Option<String>,
    },
}

/// A signal's computed cause heat score — how much independent community attention
/// exists in its semantic neighborhood.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CauseHeatScore {
    pub signal_id: Uuid,
    pub label: String,
    pub cause_heat: f64,
}

/// Source changes — observable facts about a source (moved from rootsignal-world).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum SourceChange {
    Weight {
        old: f64,
        new: f64,
    },
    Url {
        old: String,
        new: String,
    },
    Role {
        old: crate::types::SourceRole,
        new: crate::types::SourceRole,
    },
    Active {
        old: bool,
        new: bool,
    },
    Cadence {
        old: Option<u32>,
        new: Option<u32>,
    },
    ChannelWeight {
        channel: String,
        old: f64,
        new: f64,
    },
}

/// Diversity metrics for a single signal node, computed from Citation evidence edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDiversityScore {
    pub signal_id: Uuid,
    pub label: String,
    pub source_diversity: i64,
    pub channel_diversity: i64,
    pub external_ratio: f64,
}

/// Actor signal count, computed from ACTED_IN edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorStatScore {
    pub actor_id: Uuid,
    pub signal_count: u32,
}

/// A similarity edge between two signals, computed from cosine similarity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityEdge {
    pub from_id: Uuid,
    pub to_id: Uuid,
    pub weight: f64,
}

/// A tag with its computed weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagFact {
    pub slug: String,
    pub name: String,
    pub weight: f64,
}

/// A typed change to a Situation entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum SituationChange {
    Headline {
        old: String,
        new: String,
    },
    Lede {
        old: String,
        new: String,
    },
    Arc {
        old: SituationArc,
        new: SituationArc,
    },
    Temperature {
        old: f64,
        new: f64,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Category {
        old: Option<String>,
        new: Option<String>,
    },
    StructuredState {
        old: String,
        new: String,
    },
}

// ---------------------------------------------------------------------------
// Per-entity correction enums — each only has fields that exist on that type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum GatheringCorrection {
    Title {
        old: String,
        new: String,
    },
    Summary {
        old: String,
        new: String,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    Schedule {
        old: Option<Schedule>,
        new: Option<Schedule>,
    },
    Organizer {
        old: Option<String>,
        new: Option<String>,
    },
    ActionUrl {
        old: Option<String>,
        new: Option<String>,
    },
    /// Absorbs unknown/removed correction fields during deserialization (e.g. historical
    /// "confidence", "severity", "source_authority" variants removed in the taxonomy refactor).
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum ResourceCorrection {
    Title {
        old: String,
        new: String,
    },
    Summary {
        old: String,
        new: String,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    ActionUrl {
        old: Option<String>,
        new: Option<String>,
    },
    Availability {
        old: Option<String>,
        new: Option<String>,
    },
    IsOngoing {
        old: Option<bool>,
        new: Option<bool>,
    },
    /// Absorbs unknown/removed correction fields during deserialization.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum HelpRequestCorrection {
    Title {
        old: String,
        new: String,
    },
    Summary {
        old: String,
        new: String,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    Urgency {
        old: Option<Urgency>,
        new: Option<Urgency>,
    },
    WhatNeeded {
        old: Option<String>,
        new: Option<String>,
    },
    StatedGoal {
        old: Option<String>,
        new: Option<String>,
    },
    /// Absorbs unknown/removed correction fields during deserialization.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum AnnouncementCorrection {
    Title {
        old: String,
        new: String,
    },
    Summary {
        old: String,
        new: String,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    Category {
        old: Option<String>,
        new: Option<String>,
    },
    EffectiveDate {
        old: Option<DateTime<Utc>>,
        new: Option<DateTime<Utc>>,
    },
    /// Absorbs unknown/removed correction fields during deserialization.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum ConcernCorrection {
    Title {
        old: String,
        new: String,
    },
    Summary {
        old: String,
        new: String,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    Opposing {
        old: Option<String>,
        new: Option<String>,
    },
    /// Absorbs unknown/removed correction fields during deserialization.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum ConditionCorrection {
    Title {
        old: String,
        new: String,
    },
    Summary {
        old: String,
        new: String,
    },
    Sensitivity {
        old: SensitivityLevel,
        new: SensitivityLevel,
    },
    Location {
        old: Option<Location>,
        new: Option<Location>,
    },
    Subject {
        old: Option<String>,
        new: Option<String>,
    },
    ObservedBy {
        old: Option<String>,
        new: Option<String>,
    },
    Measurement {
        old: Option<String>,
        new: Option<String>,
    },
    AffectedScope {
        old: Option<String>,
        new: Option<String>,
    },
    /// Absorbs unknown/removed correction fields during deserialization.
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------
// The Event wrapper — thin dispatch to three layers
// ---------------------------------------------------------------------------

/// Wrapper event that delegates to the three event layers.
///
/// Serialization preserves the inner enum's `#[serde(tag = "type")]` format.
/// Deserialization uses `untagged` — tries WorldEvent first (most common),
/// then SystemEvent, then TelemetryEvent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    World(WorldEvent),
    System(SystemEvent),
    Telemetry(TelemetryEvent),
}

impl Event {
    /// The durable event name — `"prefix:variant"` format (e.g. `"world:gathering_announced"`).
    pub fn event_type(&self) -> &str {
        use seesaw_core::event::Event as _;
        match self {
            Event::World(w) => w.durable_name(),
            Event::System(s) => s.durable_name(),
            Event::Telemetry(t) => t.durable_name(),
        }
    }

    /// Serialize this event to a JSON Value for the EventStore payload.
    pub fn to_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("Event serialization should never fail")
    }

    /// Deserialize an event from a JSON payload.
    ///
    /// Tries WorldEvent → SystemEvent → TelemetryEvent (via `#[serde(untagged)]`).
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}

// ---------------------------------------------------------------------------
// EventDomain — compile-time exhaustive routing for projectors/handlers
// ---------------------------------------------------------------------------

/// Domain classification for event routing.
///
/// Every event belongs to exactly one domain. Projectors and handlers match on
/// this enum — Rust forces exhaustive arms, so adding a new domain variant
/// produces compile errors wherever routing decisions are made.
///
/// Unprefixed events (World, System, Telemetry) are classified as `Fact`.
/// Domain-coordination events use a `"domain:variant"` string prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventDomain {
    /// World, System, or Telemetry fact — projectable to the graph.
    Fact,
    /// `discovery:*` — source proposals, link promotion, topic collection.
    Discovery,
    /// `scrape:*` — URL resolution, fetch/extract coordination.
    Scrape,
    /// `signal:*` — dedup verdicts, signal creation, edge wiring.
    Signal,
    /// `lifecycle:*` — run start/end, phase transitions, scheduling.
    Lifecycle,
    /// `enrichment:*` — actor extraction, diversity, post-scrape enrichment.
    Enrichment,
    /// `expansion:*` — mid-run source expansion.
    Expansion,
    /// `pipeline:*` — handler-skip/fail bookkeeping.
    Pipeline,
    /// `synthesis:*` — cross-signal analysis, response mapping.
    Synthesis,
    /// `situation_weaving:*` — signal-to-situation assignment.
    SituationWeaving,
    /// `supervisor:*` — region supervision.
    Supervisor,
    /// `scheduling:*` — deferred scrapes, re-run scheduling.
    Scheduling,
}

impl EventDomain {
    /// Parse the domain from an `event_type` string (as stored in the event store).
    ///
    /// Returns `None` for genuinely unknown prefixes — callers should warn on
    /// `None` since it indicates a new domain was added without updating this enum.
    pub fn from_event_type(event_type: &str) -> Option<Self> {
        match event_type.split_once(':') {
            Some((prefix, _)) => match prefix {
                "world" | "system" | "telemetry" => Some(Self::Fact),
                "discovery" => Some(Self::Discovery),
                "scrape" => Some(Self::Scrape),
                "signal" => Some(Self::Signal),
                "lifecycle" => Some(Self::Lifecycle),
                "enrichment" => Some(Self::Enrichment),
                "expansion" => Some(Self::Expansion),
                "pipeline" => Some(Self::Pipeline),
                "synthesis" => Some(Self::Synthesis),
                "situation_weaving" => Some(Self::SituationWeaving),
                "supervisor" => Some(Self::Supervisor),
                "scheduling" => Some(Self::Scheduling),
                _ => None,
            },
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience constructors for ergonomic wrapping
// ---------------------------------------------------------------------------

impl From<WorldEvent> for Event {
    fn from(w: WorldEvent) -> Self {
        Event::World(w)
    }
}

impl From<SystemEvent> for Event {
    fn from(s: SystemEvent) -> Self {
        Event::System(s)
    }
}

impl From<TelemetryEvent> for Event {
    fn from(t: TelemetryEvent) -> Self {
        Event::Telemetry(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::SensitivityLevel;
    use crate::types::{Entity, EntityType};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn world_event_roundtrips_through_wrapper() {
        let world = WorldEvent::GatheringAnnounced {
            id: Uuid::new_v4(),
            title: "Community Cleanup".into(),
            summary: "Monthly neighborhood cleanup".into(),
            url: "https://example.com/cleanup".into(),
            published_at: Some(Utc::now()),
            extraction_id: None,
            locations: vec![Location {
                point: Some(crate::types::GeoPoint {
                    lat: 44.9778,
                    lng: -93.265,
                    precision: crate::types::GeoPrecision::Neighborhood,
                }),
                name: Some("Minneapolis".into()),
                address: None,
                role: None,
            }],
            mentioned_entities: vec![Entity {
                name: "Lake Street Council".into(),
                entity_type: EntityType::Organization,
                role: None,
            }],
            references: vec![],
            schedule: Some(Schedule {
                starts_at: Some(Utc::now()),
                ends_at: None,
                all_day: false,
                rrule: Some("FREQ=MONTHLY;BYDAY=1SA".into()),
                timezone: Some("America/Chicago".into()),
                schedule_text: None,
                rdates: vec![],
                exdates: vec![],
            }),
            action_url: Some("https://example.com/signup".into()),
        };

        let event = Event::World(world);
        let payload = event.to_payload();
        assert_eq!(payload["type"].as_str().unwrap(), "gathering_announced");

        let roundtripped = Event::from_payload(&payload).unwrap();
        assert_eq!(roundtripped.event_type(), "world:gathering_announced");

        match roundtripped {
            Event::World(WorldEvent::GatheringAnnounced {
                title,
                schedule,
                ..
            }) => {
                assert_eq!(title, "Community Cleanup");
                assert!(schedule.is_some());
                assert_eq!(schedule.unwrap().rrule.unwrap(), "FREQ=MONTHLY;BYDAY=1SA");
            }
            _ => panic!("Expected Event::World(WorldEvent::GatheringAnnounced)"),
        }
    }

    #[test]
    fn system_event_roundtrips_through_wrapper() {
        let system = SystemEvent::SensitivityClassified {
            signal_id: Uuid::new_v4(),
            level: SensitivityLevel::Sensitive,
        };

        let event = Event::System(system);
        let payload = event.to_payload();
        assert_eq!(payload["type"].as_str().unwrap(), "sensitivity_classified");

        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::System(SystemEvent::SensitivityClassified { level, .. }) => {
                assert_eq!(level, SensitivityLevel::Sensitive);
            }
            _ => panic!("Expected Event::System(SystemEvent::SensitivityClassified)"),
        }
    }

    #[test]
    fn telemetry_event_roundtrips_through_wrapper() {
        let telemetry = TelemetryEvent::UrlScraped {
            url: "https://example.com".into(),
            strategy: "web_page".into(),
            success: true,
            content_bytes: 1024,
        };

        let event = Event::Telemetry(telemetry);
        let payload = event.to_payload();
        assert_eq!(payload["type"].as_str().unwrap(), "url_scraped");

        let roundtripped = Event::from_payload(&payload).unwrap();
        assert_eq!(roundtripped.event_type(), "telemetry:url_scraped");
    }

    #[test]
    fn implied_queries_extracted_roundtrips() {
        let system = SystemEvent::ImpliedQueriesExtracted {
            signal_id: Uuid::new_v4(),
            queries: vec!["cleanup Minneapolis".into(), "volunteer events".into()],
        };

        let event = Event::System(system);
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::System(SystemEvent::ImpliedQueriesExtracted { queries, .. }) => {
                assert_eq!(queries.len(), 2);
                assert_eq!(queries[0], "cleanup Minneapolis");
            }
            _ => panic!("Expected ImpliedQueriesExtracted"),
        }
    }

    #[test]
    fn source_change_nested_enum_roundtrip() {
        let system = SystemEvent::SourceChanged {
            source_id: Uuid::new_v4(),
            canonical_key: "web:example.com".into(),
            change: SourceChange::Weight { old: 0.5, new: 0.8 },
        };

        let event = Event::System(system);
        let payload = event.to_payload();
        let json_change = &payload["change"];
        assert_eq!(json_change["field"].as_str().unwrap(), "weight");

        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::System(SystemEvent::SourceChanged { change, .. }) => match change {
                SourceChange::Weight { old, new } => {
                    assert!((old - 0.5).abs() < f64::EPSILON);
                    assert!((new - 0.8).abs() < f64::EPSILON);
                }
                _ => panic!("Expected SourceChange::Weight"),
            },
            _ => panic!("Expected Event::System(SystemEvent::SourceChanged)"),
        }
    }

    #[test]
    fn situation_change_nested_enum_roundtrip() {
        let system = SystemEvent::SituationChanged {
            situation_id: Uuid::new_v4(),
            change: SituationChange::Arc {
                old: crate::types::SituationArc::Emerging,
                new: crate::types::SituationArc::Developing,
            },
        };

        let event = Event::System(system);
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::System(SystemEvent::SituationChanged {
                change: SituationChange::Arc { old, new },
                ..
            }) => {
                assert_eq!(old, crate::types::SituationArc::Emerging);
                assert_eq!(new, crate::types::SituationArc::Developing);
            }
            _ => panic!("Expected SituationChanged::Arc"),
        }
    }

    #[test]
    fn gathering_correction_roundtrip() {
        let system = SystemEvent::GatheringCorrected {
            signal_id: Uuid::new_v4(),
            correction: GatheringCorrection::Title {
                old: "Commuinty Cleanup".into(),
                new: "Community Cleanup".into(),
            },
            reason: "Typo in title".into(),
        };

        let event = Event::System(system);
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::System(SystemEvent::GatheringCorrected {
                correction: GatheringCorrection::Title { old, new },
                reason,
                ..
            }) => {
                assert_eq!(old, "Commuinty Cleanup");
                assert_eq!(new, "Community Cleanup");
                assert_eq!(reason, "Typo in title");
            }
            _ => panic!("Expected GatheringCorrected::Title"),
        }
    }

    #[test]
    fn from_impls_work() {
        let w: Event = SystemEvent::ObservationCorroborated {
            signal_id: Uuid::new_v4(),
            node_type: crate::types::NodeType::Gathering,
            new_url: "test".into(),
            summary: None,
        }
        .into();
        assert_eq!(w.event_type(), "system:observation_corroborated");

        let s: Event = SystemEvent::SignalsExpired {
            signals: vec![crate::system_events::StaleSignal {
                signal_id: Uuid::new_v4(),
                node_type: crate::types::NodeType::Gathering,
                reason: "test".into(),
            }],
        }
        .into();
        assert_eq!(s.event_type(), "system:signals_expired");

        let t: Event = TelemetryEvent::BudgetCheckpoint {
            spent_cents: 100,
            remaining_cents: 900,
        }
        .into();
        assert_eq!(t.event_type(), "telemetry:budget_checkpoint");
    }

    #[test]
    fn schedule_optional_fields_deserialize_from_minimal_json() {
        let json = serde_json::json!({
            "starts_at": "2026-03-08T19:00:00Z"
        });
        let schedule: Schedule = serde_json::from_value(json).unwrap();
        assert!(schedule.starts_at.is_some());
        assert!(schedule.ends_at.is_none());
        assert!(!schedule.all_day);
        assert!(schedule.rrule.is_none());
        assert!(schedule.timezone.is_none());
    }

    #[test]
    fn location_optional_fields_deserialize_from_minimal_json() {
        let json = serde_json::json!({
            "name": "Lake Harriet"
        });
        let loc: Location = serde_json::from_value(json).unwrap();
        assert!(loc.point.is_none());
        assert_eq!(loc.name.unwrap(), "Lake Harriet");
        assert!(loc.address.is_none());
    }
}
