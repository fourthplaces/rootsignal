//! Pipeline state managed by the reducer.
//!
//! `PipelineState` is the mutable state for a scout run. State mutations
//! happen in the reducer (pure, synchronous), not scattered across handlers.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rootsignal_common::types::ActorContext;
use rootsignal_common::ScoutScope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_common::types::SourceNode;
use rootsignal_common::Node;

use crate::enrichment::link_promoter::CollectedLink;
use crate::infra::embedder::TextEmbedder;
use crate::infra::util::sanitize_url;
use crate::pipeline::extractor::ResourceTag;
use crate::pipeline::scrape_phase::EmbeddingCache;
use crate::pipeline::stats::ScoutStats;
use crate::traits::SignalStore;

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

// ---------------------------------------------------------------------------
// PipelineDeps — immutable dependencies for the router
// ---------------------------------------------------------------------------

/// Immutable dependencies passed to `Engine::dispatch()`.
///
/// Does NOT include the GraphProjector — that lives on the ScoutRouter,
/// since only the router (not individual handlers) needs to project
/// World/System events to Neo4j.
pub struct PipelineDeps {
    pub store: Arc<dyn SignalStore>,
    pub embedder: Arc<dyn TextEmbedder>,
    pub region: ScoutScope,
    pub run_id: String,
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
