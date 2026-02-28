//! Pipeline state managed by the aggregate.
//!
//! `PipelineState` is the mutable state for a scout run. State mutations
//! happen in `apply()` (pure, synchronous), not scattered across handlers.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use rootsignal_common::types::{ActorContext, NodeType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_common::types::SourceNode;
use rootsignal_common::Node;

use crate::enrichment::link_promoter::CollectedLink;
use crate::infra::util::sanitize_url;
use crate::pipeline::extractor::ResourceTag;
use crate::pipeline::scrape_phase::EmbeddingCache;
use crate::core::events::{FreshnessBucket, PipelineEvent, ScoutEvent};
use crate::core::stats::ScoutStats;

/// Mutable state for a scout run, updated by the reducer.
pub struct PipelineState {
    /// In-memory embedding cache for cross-batch dedup (layer 1 of 4).
    pub embed_cache: EmbeddingCache,

    /// URL → source canonical_key resolution map.
    pub url_to_canonical_key: HashMap<String, String>,

    /// Per-source signal counts (canonical_key → count).
    pub source_signal_counts: HashMap<String, u32>,

    /// Expansion queries extracted from signals.
    pub expansion_queries: Vec<String>,

    /// Social topics for discovery.
    pub social_expansion_topics: Vec<String>,

    /// Aggregated run metrics.
    pub stats: ScoutStats,

    /// Canonical keys where the query API errored.
    pub query_api_errors: HashSet<String>,

    /// Actor context keyed by source canonical_key.
    pub actor_contexts: HashMap<String, ActorContext>,

    /// RSS/Atom pub_date keyed by article URL, used as fallback published_at.
    pub url_to_pub_date: HashMap<String, DateTime<Utc>>,

    /// Links collected during scraping for promotion.
    pub collected_links: Vec<CollectedLink>,

    /// Extracted node batches awaiting dedup, keyed by source URL.
    /// Stashed before `SignalsExtracted` is dispatched, consumed by the dedup handler.
    pub extracted_batches: HashMap<String, ExtractedBatch>,

    /// Nodes awaiting creation (passed dedup as new).
    /// Stashed by the dedup handler, consumed by `handle_create`,
    /// which moves wiring data to `wiring_contexts`.
    pub pending_nodes: HashMap<Uuid, PendingNode>,

    /// Edge-wiring context stashed by `handle_create` for `handle_signal_stored`.
    /// Separate from `pending_nodes` so each handler has a clear lifecycle:
    /// dedup stashes → create consumes + stashes wiring → signal_stored consumes.
    pub wiring_contexts: HashMap<Uuid, WiringContext>,
}

/// A batch of extracted nodes for a single URL, awaiting dedup.
/// Stashed in state before `SignalsExtracted` is dispatched.
pub struct ExtractedBatch {
    pub content: String,
    pub nodes: Vec<Node>,
    pub resource_tags: HashMap<Uuid, Vec<ResourceTag>>,
    pub signal_tags: HashMap<Uuid, Vec<String>>,
    pub author_actors: HashMap<Uuid, String>,
    pub source_id: Option<Uuid>,
}

/// Node data stashed by the dedup handler for the creation handler to consume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingNode {
    pub node: rootsignal_common::Node,
    #[serde(skip)]
    pub embedding: Vec<f32>,
    pub content_hash: String,
    pub resource_tags: Vec<ResourceTag>,
    pub signal_tags: Vec<String>,
    pub author_name: Option<String>,
    pub source_id: Option<Uuid>,
}

/// Edge-wiring data stashed by `handle_create` for `handle_signal_stored`.
/// Only the fields needed for wiring — the Node itself is already projected.
pub struct WiringContext {
    pub resource_tags: Vec<ResourceTag>,
    pub signal_tags: Vec<String>,
    pub author_name: Option<String>,
    pub source_id: Option<Uuid>,
}

impl PipelineState {
    pub fn new(url_to_canonical_key: HashMap<String, String>) -> Self {
        Self {
            embed_cache: EmbeddingCache::new(),
            url_to_canonical_key,
            source_signal_counts: HashMap::new(),
            expansion_queries: Vec::new(),
            social_expansion_topics: Vec::new(),
            stats: ScoutStats::default(),
            query_api_errors: HashSet::new(),
            actor_contexts: HashMap::new(),
            url_to_pub_date: HashMap::new(),
            collected_links: Vec::new(),
            extracted_batches: HashMap::new(),
            pending_nodes: HashMap::new(),
            wiring_contexts: HashMap::new(),
        }
    }

    /// Build from source nodes — resolves URL → canonical_key mappings.
    pub fn from_sources(sources: &[SourceNode]) -> Self {
        let url_to_canonical_key = sources
            .iter()
            .filter_map(|s| {
                s.url
                    .as_ref()
                    .map(|u| (sanitize_url(u), s.canonical_key.clone()))
            })
            .collect();
        Self::new(url_to_canonical_key)
    }

    /// Rebuild known URLs from current URL map state.
    pub fn known_urls(&self) -> HashSet<String> {
        self.url_to_canonical_key.keys().cloned().collect()
    }
}

impl Default for PipelineState {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

impl seesaw_core::Aggregate for PipelineState {
    type Event = ScoutEvent;

    fn aggregate_type() -> &'static str {
        "scout_pipeline"
    }

    fn apply(&mut self, event: ScoutEvent) {
        let ScoutEvent::Pipeline(ref pe) = event else {
            // World and System events don't update pipeline state.
            return;
        };

        match pe {
            // Content fetching
            PipelineEvent::ContentFetched { .. } => {
                self.stats.urls_scraped += 1;
            }
            PipelineEvent::ContentUnchanged { .. } => {
                self.stats.urls_unchanged += 1;
            }
            PipelineEvent::ContentFetchFailed { .. } => {
                self.stats.urls_failed += 1;
            }

            // Extraction
            PipelineEvent::SignalsExtracted { count, .. } => {
                self.stats.signals_extracted += count;
            }

            // Dedup verdicts
            PipelineEvent::NewSignalAccepted {
                node_id,
                node_type,
                pending_node,
                ..
            } => {
                self.stats.signals_stored += 1;
                if let Some(idx) = signal_type_index(node_type) {
                    self.stats.by_type[idx] += 1;
                }
                self.wiring_contexts.insert(
                    *node_id,
                    WiringContext {
                        resource_tags: pending_node.resource_tags.clone(),
                        signal_tags: pending_node.signal_tags.clone(),
                        author_name: pending_node.author_name.clone(),
                        source_id: pending_node.source_id,
                    },
                );
                self.pending_nodes.insert(*node_id, *pending_node.clone());
            }
            PipelineEvent::CrossSourceMatchDetected { .. }
            | PipelineEvent::SameSourceReencountered { .. } => {
                self.stats.signals_deduplicated += 1;
            }

            // URL-level summary
            PipelineEvent::UrlProcessed {
                canonical_key,
                signals_created,
                ..
            } => {
                *self
                    .source_signal_counts
                    .entry(canonical_key.clone())
                    .or_default() += signals_created;
            }

            // Links
            PipelineEvent::LinkCollected { url, discovered_on } => {
                self.collected_links.push(CollectedLink {
                    url: url.clone(),
                    discovered_on: discovered_on.clone(),
                });
            }

            // Expansion
            PipelineEvent::ExpansionQueryCollected { query, .. } => {
                self.expansion_queries.push(query.clone());
                self.stats.expansion_queries_collected += 1;
            }
            PipelineEvent::SocialTopicCollected { topic } => {
                self.social_expansion_topics.push(topic.clone());
                self.stats.expansion_social_topics_queued += 1;
            }

            // Social
            PipelineEvent::SocialPostsFetched { count, .. } => {
                self.stats.social_media_posts += count;
            }

            // Freshness
            PipelineEvent::FreshnessRecorded { bucket, .. } => match bucket {
                FreshnessBucket::Within7d => self.stats.fresh_7d += 1,
                FreshnessBucket::Within30d => self.stats.fresh_30d += 1,
                FreshnessBucket::Within90d => self.stats.fresh_90d += 1,
                FreshnessBucket::Older | FreshnessBucket::Unknown => {}
            },

            // SignalReaderd — clean up PendingNode
            PipelineEvent::SignalReaderd { node_id, .. } => {
                self.pending_nodes.remove(node_id);
            }

            // DedupCompleted — clean up extracted batch
            PipelineEvent::DedupCompleted { url } => {
                self.extracted_batches.remove(url);
            }

            // LinksPromoted — clear collected links (they've been promoted to sources)
            PipelineEvent::LinksPromoted { .. } => {
                self.collected_links.clear();
            }

            // Phase lifecycle / engine lifecycle — no state changes
            PipelineEvent::PhaseStarted { .. }
            | PipelineEvent::PhaseCompleted { .. }
            | PipelineEvent::ExtractionFailed { .. }
            | PipelineEvent::ActorEnrichmentCompleted { .. }
            | PipelineEvent::EngineStarted { .. } => {}

            PipelineEvent::SourceDiscovered { .. } => {
                self.stats.sources_discovered += 1;
            }
        }
    }
}

/// Map signal node types to the `by_type` stats index.
/// Returns None for non-signal types (e.g. Citation).
fn signal_type_index(nt: &NodeType) -> Option<usize> {
    match nt {
        NodeType::Gathering => Some(0),
        NodeType::Aid => Some(1),
        NodeType::Need => Some(2),
        NodeType::Notice => Some(3),
        NodeType::Tension => Some(4),
        NodeType::Citation => None,
    }
}
