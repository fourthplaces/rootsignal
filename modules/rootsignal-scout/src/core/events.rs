//! Scout event types for the engine dispatch loop.
//!
//! `ScoutEvent` wraps three layers:
//! - **Pipeline**: internal bookkeeping (scrape, extract, dedup, store)
//! - **World**: observable facts (discoveries, citations, actors)
//! - **System**: editorial decisions (sensitivity, corrections, sources)
//!
//! Plus domain event wrappers for infrastructure handler interop:
//! - **Lifecycle**: phase transitions, run lifecycle
//! - **Signal**: dedup verdicts, signal storage
//! - **Discovery**: source discovery, link promotion
//! - **Enrichment**: actor enrichment
//!
//! All variants flow through the same engine dispatch loop,
//! get persisted to the EventStore, and form causal chains.

use chrono::{DateTime, Utc};
use rootsignal_common::events::{Event, Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::types::SourceNode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::signals::events::SignalEvent;

// ---------------------------------------------------------------------------
// ScoutEvent — the unified event type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "layer")]
pub enum ScoutEvent {
    Pipeline(PipelineEvent),
    World(WorldEvent),
    System(SystemEvent),
    // Domain event wrappers — used by infrastructure handlers (persist, reduce,
    // capture) to handle per-domain events dispatched through the engine.
    Lifecycle(LifecycleEvent),
    Signal(SignalEvent),
    Discovery(DiscoveryEvent),
    Enrichment(EnrichmentEvent),
}

impl ScoutEvent {
    /// Whether this event needs graph projection.
    pub fn is_projectable(&self) -> bool {
        match self {
            ScoutEvent::World(_) | ScoutEvent::System(_) => true,
            ScoutEvent::Pipeline(pe) => pe.is_projectable(),
            ScoutEvent::Discovery(de) => de.is_projectable(),
            ScoutEvent::Lifecycle(_) | ScoutEvent::Signal(_) | ScoutEvent::Enrichment(_) => false,
        }
    }

    pub fn event_type_str(&self) -> String {
        match self {
            ScoutEvent::Pipeline(pe) => format!("pipeline:{}", pe.variant_name()),
            ScoutEvent::World(we) => we.event_type().to_string(),
            ScoutEvent::System(se) => se.event_type().to_string(),
            ScoutEvent::Lifecycle(le) => le.event_type_str(),
            ScoutEvent::Signal(se) => se.event_type_str(),
            ScoutEvent::Discovery(de) => de.event_type_str(),
            ScoutEvent::Enrichment(ee) => ee.event_type_str(),
        }
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        match self {
            // Pipeline events serialize normally — projector skips "pipeline:*".
            ScoutEvent::Pipeline(pe) => {
                serde_json::to_value(pe).expect("PipelineEvent serialization should never fail")
            }
            // World/System serialize in projector-compatible format:
            // just the inner event via Event's to_payload(), not the tagged ScoutEvent wrapper.
            ScoutEvent::World(we) => Event::World(we.clone()).to_payload(),
            ScoutEvent::System(se) => Event::System(se.clone()).to_payload(),
            // Domain events serialize directly.
            ScoutEvent::Lifecycle(le) => le.to_persist_payload(),
            ScoutEvent::Signal(se) => se.to_persist_payload(),
            ScoutEvent::Discovery(de) => de.to_persist_payload(),
            ScoutEvent::Enrichment(ee) => ee.to_persist_payload(),
        }
    }
}

// ---------------------------------------------------------------------------
// PipelineEvent — internal pipeline bookkeeping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
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
        /// The extracted batch, carried as event payload for the dedup handler.
        batch: Box<crate::core::aggregate::ExtractedBatch>,
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

    // Link promotion
    LinksPromoted {
        count: u32,
    },

    // Actor enrichment
    ActorEnrichmentCompleted {
        actors_updated: u32,
    },
}

impl PipelineEvent {
    /// Whether this pipeline event needs graph projection.
    pub fn is_projectable(&self) -> bool {
        matches!(self, PipelineEvent::SourceDiscovered { .. })
    }

    pub fn variant_name(&self) -> &'static str {
        match self {
            PipelineEvent::ContentFetched { .. } => "content_fetched",
            PipelineEvent::ContentUnchanged { .. } => "content_unchanged",
            PipelineEvent::ContentFetchFailed { .. } => "content_fetch_failed",
            PipelineEvent::SignalsExtracted { .. } => "signals_extracted",
            PipelineEvent::LinkCollected { .. } => "link_collected",
            PipelineEvent::ExpansionQueryCollected { .. } => "expansion_query_collected",
            PipelineEvent::SocialTopicCollected { .. } => "social_topic_collected",
            PipelineEvent::SourceDiscovered { .. } => "source_discovered",
            PipelineEvent::SocialPostsFetched { .. } => "social_posts_fetched",
            PipelineEvent::FreshnessRecorded { .. } => "freshness_recorded",
            PipelineEvent::LinksPromoted { .. } => "links_promoted",
            PipelineEvent::ActorEnrichmentCompleted { .. } => "actor_enrichment_completed",
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
    Synthesis,
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

// Seesaw event upcasting — version 1, no schema migrations needed yet.
// impl_upcast! removed in seesaw_core 0.13.0 (no longer needed for in-memory engine).

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
        let pipeline_projectable = ScoutEvent::Pipeline(PipelineEvent::SourceDiscovered {
            source,
            discovered_by: "test".into(),
        });
        assert!(pipeline_projectable.is_projectable());

        let pipeline_not = ScoutEvent::Pipeline(PipelineEvent::ContentUnchanged {
            url: "https://example.org".into(),
            canonical_key: "example.org".into(),
        });
        assert!(!pipeline_not.is_projectable());
    }
}
