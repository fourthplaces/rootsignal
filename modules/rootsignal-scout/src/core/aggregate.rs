//! Pipeline state managed by the aggregate + handler stash.
//!
//! `PipelineState` is the mutable state for a scout run. State mutations
//! happen in per-domain `apply_*` methods (pure, synchronous), not
//! scattered across handlers.
//!

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use rootsignal_common::types::{ActorContext, NodeType, SourceNode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_common::Node;

use crate::core::events::FreshnessBucket;
use crate::core::stats::ScoutStats;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::SignalEvent;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::infra::util::sanitize_url;
use crate::core::extractor::ResourceTag;
use crate::core::embedding_cache::EmbeddingCache;

/// Scheduling data passed between schedule_handler and scrape handlers.
pub struct ScheduledData {
    pub all_sources: Vec<SourceNode>,
    pub scheduled_sources: Vec<SourceNode>,
    pub tension_phase_keys: HashSet<String>,
    pub response_phase_keys: HashSet<String>,
    pub scheduled_keys: HashSet<String>,
    pub consumed_pin_ids: Vec<Uuid>,
}

/// Accumulated output from the schedule phase.
pub struct ScheduleOutput {
    pub scheduled_data: ScheduledData,
    pub actor_contexts: HashMap<String, ActorContext>,
    pub url_mappings: HashMap<String, String>,
    pub tension_count: u32,
    pub response_count: u32,
}

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

    /// Nodes awaiting creation (passed dedup as new).
    /// Stashed by the dedup handler, consumed by `create_signal_events`,
    /// which moves wiring data to `wiring_contexts`.
    pub pending_nodes: HashMap<Uuid, PendingNode>,

    /// Edge-wiring context stashed by `create_signal_events` for `wire_signal_edges`.
    /// Separate from `pending_nodes` so each handler has a clear lifecycle:
    /// dedup stashes → create consumes + stashes wiring → signal_stored consumes.
    pub wiring_contexts: HashMap<Uuid, WiringContext>,

    /// Scheduling data stashed by schedule_handler, consumed by scrape handlers.
    pub scheduled: Option<ScheduledData>,

    /// Social topics collected during mid-run discovery, consumed by response scrape.
    pub social_topics: Vec<String>,
}

/// A batch of extracted nodes for a single URL, carried on `SignalsExtracted`
/// as event payload for the dedup handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Edge-wiring data stashed by `create_signal_events` for `wire_signal_edges`.
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
            pending_nodes: HashMap::new(),
            wiring_contexts: HashMap::new(),
            scheduled: None,
            social_topics: Vec::new(),
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

impl PipelineState {
    /// Apply a scrape domain event.
    pub fn apply_scrape(&mut self, event: &ScrapeEvent) {
        match event {
            ScrapeEvent::ContentFetched { .. } => {
                self.stats.urls_scraped += 1;
            }
            ScrapeEvent::ContentUnchanged { .. } => {
                self.stats.urls_unchanged += 1;
            }
            ScrapeEvent::ContentFetchFailed { .. } => {
                self.stats.urls_failed += 1;
            }
            ScrapeEvent::SignalsExtracted { count, .. } => {
                self.stats.signals_extracted += count;
            }
            ScrapeEvent::SocialPostsFetched { count, .. } => {
                self.stats.social_media_posts += count;
            }
            ScrapeEvent::FreshnessRecorded { bucket, .. } => match bucket {
                FreshnessBucket::Within7d => self.stats.fresh_7d += 1,
                FreshnessBucket::Within30d => self.stats.fresh_30d += 1,
                FreshnessBucket::Within90d => self.stats.fresh_90d += 1,
                FreshnessBucket::Older | FreshnessBucket::Unknown => {}
            },
            ScrapeEvent::LinkCollected {
                url, discovered_on, ..
            } => {
                self.collected_links.push(CollectedLink {
                    url: url.clone(),
                    discovered_on: discovered_on.clone(),
                });
            }
            ScrapeEvent::ExtractionFailed { .. } => {}
        }
    }

    /// Apply a signal domain event.
    pub fn apply_signal(&mut self, event: &SignalEvent) {
        match event {
            SignalEvent::SignalsExtracted { count, .. } => {
                self.stats.signals_extracted += count;
            }
            SignalEvent::NewSignalAccepted {
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
            SignalEvent::CrossSourceMatchDetected { .. }
            | SignalEvent::SameSourceReencountered { .. } => {
                self.stats.signals_deduplicated += 1;
            }
            SignalEvent::UrlProcessed {
                canonical_key,
                signals_created,
                ..
            } => {
                *self
                    .source_signal_counts
                    .entry(canonical_key.clone())
                    .or_default() += signals_created;
            }
            SignalEvent::SignalCreated { node_id, .. } => {
                self.pending_nodes.remove(node_id);
            }
            SignalEvent::DedupCompleted { .. } => {}
        }
    }

    /// Apply a discovery domain event.
    pub fn apply_discovery(&mut self, event: &DiscoveryEvent) {
        match event {
            DiscoveryEvent::SourceDiscovered { .. } => {
                self.stats.sources_discovered += 1;
            }
            DiscoveryEvent::LinksPromoted { .. } => {
                self.collected_links.clear();
            }
            DiscoveryEvent::ExpansionQueryCollected { query, .. } => {
                self.expansion_queries.push(query.clone());
                self.stats.expansion_queries_collected += 1;
            }
            DiscoveryEvent::SocialTopicCollected { topic, .. } => {
                self.social_expansion_topics.push(topic.clone());
                self.stats.expansion_social_topics_queued += 1;
            }
        }
    }

    // LifecycleEvent and EnrichmentEvent are no-ops for aggregate state.

    // -----------------------------------------------------------------
    // Apply accumulated outputs from pure activity functions
    // -----------------------------------------------------------------

    /// Apply accumulated scrape output to pipeline state.
    pub fn apply_scrape_output(&mut self, output: crate::domains::scrape::activities::scrape_phase::ScrapeOutput) {
        self.url_to_canonical_key.extend(output.url_mappings);
        for (k, v) in output.source_signal_counts {
            *self.source_signal_counts.entry(k).or_default() += v;
        }
        self.query_api_errors.extend(output.query_api_errors);
        self.url_to_pub_date.extend(output.pub_dates);
        self.collected_links.extend(output.collected_links);
        self.expansion_queries.extend(output.expansion_queries);
        self.stats.social_media_posts += output.stats_delta.social_media_posts;
        self.stats.discovery_posts_found += output.stats_delta.discovery_posts_found;
        self.stats.discovery_accounts_found += output.stats_delta.discovery_accounts_found;
    }

    /// Apply accumulated expansion output to pipeline state.
    pub fn apply_expansion_output(&mut self, output: crate::domains::expansion::activities::expansion::ExpansionOutput) {
        self.social_expansion_topics
            .extend(output.social_expansion_topics);
        self.stats.expansion_deferred_expanded = output.expansion_deferred_expanded;
        self.stats.expansion_queries_collected = output.expansion_queries_collected;
        self.stats.expansion_sources_created = output.expansion_sources_created;
        self.stats.expansion_social_topics_queued = output.expansion_social_topics_queued;
    }

    /// Apply schedule output: stash scheduled data, actor contexts, URL mappings.
    pub fn apply_schedule_output(&mut self, output: ScheduleOutput) {
        self.actor_contexts.extend(output.actor_contexts);
        self.url_to_canonical_key.extend(output.url_mappings);
        self.scheduled = Some(output.scheduled_data);
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
        NodeType::Condition => Some(5),
        NodeType::Incident => Some(6),
        NodeType::Citation => None,
    }
}
