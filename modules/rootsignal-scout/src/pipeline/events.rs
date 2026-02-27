//! Scout event types for the engine dispatch loop.
//!
//! `ScoutEvent` wraps three layers:
//! - **Pipeline**: internal bookkeeping (scrape, extract, dedup, store)
//! - **World**: observable facts (discoveries, citations, actors)
//! - **System**: editorial decisions (sensitivity, corrections, sources)
//!
//! All variants flow through the same engine dispatch loop,
//! get persisted to the EventStore, and form causal chains.

use chrono::{DateTime, Utc};
use rootsignal_common::events::{Event, Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::types::{NodeType, SourceNode};
use rootsignal_engine::EventLike;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// ScoutEvent — the unified event type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "layer")]
pub enum ScoutEvent {
    Pipeline(PipelineEvent),
    World(WorldEvent),
    System(SystemEvent),
}

impl ScoutEvent {
    /// Whether this event needs graph projection.
    pub fn is_projectable(&self) -> bool {
        match self {
            ScoutEvent::World(_) | ScoutEvent::System(_) => true,
            ScoutEvent::Pipeline(pe) => pe.is_projectable(),
        }
    }
}

impl EventLike for ScoutEvent {
    fn event_type_str(&self) -> String {
        match self {
            ScoutEvent::Pipeline(pe) => format!("pipeline:{}", pe.variant_name()),
            ScoutEvent::World(we) => we.event_type().to_string(),
            ScoutEvent::System(se) => se.event_type().to_string(),
        }
    }

    fn to_persist_payload(&self) -> serde_json::Value {
        match self {
            // Pipeline events serialize normally — projector skips "pipeline:*".
            ScoutEvent::Pipeline(pe) => {
                serde_json::to_value(pe).expect("PipelineEvent serialization should never fail")
            }
            // World/System serialize in projector-compatible format:
            // just the inner event via Event's to_payload(), not the tagged ScoutEvent wrapper.
            ScoutEvent::World(we) => Event::World(we.clone()).to_payload(),
            ScoutEvent::System(se) => Event::System(se.clone()).to_payload(),
        }
    }
}

// ---------------------------------------------------------------------------
// PipelineEvent — internal pipeline bookkeeping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    // Phase lifecycle
    PhaseStarted {
        phase: PipelinePhase,
    },
    PhaseCompleted {
        phase: PipelinePhase,
    },

    // Content fetching
    ContentFetched {
        url: String,
        canonical_key: String,
        content_hash: String,
        link_count: u32,
    },
    ContentUnchanged {
        url: String,
        canonical_key: String,
    },
    ContentFetchFailed {
        url: String,
        canonical_key: String,
        error: String,
    },

    // Extraction
    SignalsExtracted {
        url: String,
        canonical_key: String,
        count: u32,
    },
    ExtractionFailed {
        url: String,
        canonical_key: String,
        error: String,
    },

    // Dedup verdicts — facts about what the dedup layer observed
    NewSignalAccepted {
        node_id: Uuid,
        node_type: NodeType,
        title: String,
        source_url: String,
        pending_node: Box<crate::pipeline::state::PendingNode>,
    },
    CrossSourceMatchDetected {
        existing_id: Uuid,
        node_type: NodeType,
        source_url: String,
        similarity: f64,
    },
    SameSourceReencountered {
        existing_id: Uuid,
        node_type: NodeType,
        source_url: String,
        similarity: f64,
    },

    // Signal stored (after world + system events emitted)
    SignalReaderd {
        node_id: Uuid,
        node_type: NodeType,
        source_url: String,
        canonical_key: String,
    },

    // Dedup batch complete — reducer cleans up extracted batch
    DedupCompleted {
        url: String,
    },

    // URL-level summary (replaces stats diffing pattern)
    UrlProcessed {
        url: String,
        canonical_key: String,
        signals_created: u32,
        signals_deduplicated: u32,
    },

    // Link discovery
    LinkCollected {
        url: String,
        discovered_on: String,
    },

    // Expansion
    ExpansionQueryCollected {
        query: String,
        source_url: String,
    },
    SocialTopicCollected {
        topic: String,
    },

    // Source discovery
    SourceDiscovered {
        source: SourceNode,
        discovered_by: String,
    },

    // Social
    SocialPostsFetched {
        canonical_key: String,
        platform: String,
        count: u32,
    },

    // Freshness
    FreshnessRecorded {
        node_id: Uuid,
        published_at: Option<DateTime<Utc>>,
        bucket: FreshnessBucket,
    },

    // Engine lifecycle
    EngineStarted {
        run_id: String,
    },
}

impl PipelineEvent {
    /// Whether this pipeline event needs graph projection.
    pub fn is_projectable(&self) -> bool {
        matches!(self, PipelineEvent::SourceDiscovered { .. })
    }

    pub fn variant_name(&self) -> &'static str {
        match self {
            PipelineEvent::PhaseStarted { .. } => "phase_started",
            PipelineEvent::PhaseCompleted { .. } => "phase_completed",
            PipelineEvent::ContentFetched { .. } => "content_fetched",
            PipelineEvent::ContentUnchanged { .. } => "content_unchanged",
            PipelineEvent::ContentFetchFailed { .. } => "content_fetch_failed",
            PipelineEvent::SignalsExtracted { .. } => "signals_extracted",
            PipelineEvent::ExtractionFailed { .. } => "extraction_failed",
            PipelineEvent::NewSignalAccepted { .. } => "new_signal_accepted",
            PipelineEvent::CrossSourceMatchDetected { .. } => "cross_source_match_detected",
            PipelineEvent::SameSourceReencountered { .. } => "same_source_reencountered",
            PipelineEvent::SignalReaderd { .. } => "signal_stored",
            PipelineEvent::DedupCompleted { .. } => "dedup_completed",
            PipelineEvent::UrlProcessed { .. } => "url_processed",
            PipelineEvent::LinkCollected { .. } => "link_collected",
            PipelineEvent::ExpansionQueryCollected { .. } => "expansion_query_collected",
            PipelineEvent::SocialTopicCollected { .. } => "social_topic_collected",
            PipelineEvent::SourceDiscovered { .. } => "source_discovered",
            PipelineEvent::SocialPostsFetched { .. } => "social_posts_fetched",
            PipelineEvent::FreshnessRecorded { .. } => "freshness_recorded",
            PipelineEvent::EngineStarted { .. } => "engine_started",
        }
    }
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelinePhase {
    ReapExpired,
    TensionScrape,
    MidRunDiscovery,
    ResponseScrape,
    Expansion,
    SocialScrape,
    SocialDiscovery,
    ActorEnrichment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessBucket {
    Within7d,
    Within30d,
    Within90d,
    Older,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode, SourceRole};

    #[test]
    fn source_discovered_round_trips_through_persist_payload() {
        let source = SourceNode::new(
            canonical_value("https://example.org"),
            canonical_value("https://example.org"),
            Some("https://example.org".into()),
            DiscoveryMethod::LinkedFrom,
            0.25,
            SourceRole::Mixed,
            Some("test context".into()),
        );

        let event = ScoutEvent::Pipeline(PipelineEvent::SourceDiscovered {
            source: source.clone(),
            discovered_by: "link_promoter".into(),
        });

        let payload = event.to_persist_payload();

        // Deserialize through the same path the GraphProjector uses
        #[derive(serde::Deserialize)]
        struct Payload {
            source: SourceNode,
            discovered_by: String,
        }
        let round_tripped: Payload =
            serde_json::from_value(payload).expect("SourceDiscovered should round-trip");

        assert_eq!(round_tripped.source.id, source.id);
        assert_eq!(round_tripped.source.canonical_key, source.canonical_key);
        assert_eq!(round_tripped.source.url, source.url);
        assert_eq!(round_tripped.source.weight, source.weight);
        assert_eq!(round_tripped.source.gap_context, source.gap_context);
        assert_eq!(round_tripped.discovered_by, "link_promoter");
    }

    #[test]
    fn source_discovered_is_projectable() {
        let source = SourceNode::new(
            "key".into(),
            "val".into(),
            None,
            DiscoveryMethod::GapAnalysis,
            0.5,
            SourceRole::Response,
            None,
        );
        let pe = PipelineEvent::SourceDiscovered {
            source,
            discovered_by: "test".into(),
        };
        assert!(pe.is_projectable());

        let non_projectable = PipelineEvent::ContentFetched {
            url: "x".into(),
            canonical_key: "x".into(),
            content_hash: "x".into(),
            link_count: 0,
        };
        assert!(!non_projectable.is_projectable());
    }

    #[test]
    fn scout_event_projectable_delegates_correctly() {
        let source = SourceNode::new(
            "key".into(),
            "val".into(),
            None,
            DiscoveryMethod::GapAnalysis,
            0.5,
            SourceRole::Response,
            None,
        );
        let pipeline_projectable =
            ScoutEvent::Pipeline(PipelineEvent::SourceDiscovered {
                source,
                discovered_by: "test".into(),
            });
        assert!(pipeline_projectable.is_projectable());

        let pipeline_not = ScoutEvent::Pipeline(PipelineEvent::PhaseStarted {
            phase: PipelinePhase::Expansion,
        });
        assert!(!pipeline_not.is_projectable());
    }
}
