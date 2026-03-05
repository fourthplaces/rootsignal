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

use crate::core::events::{FreshnessBucket, PipelinePhase};
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::scrape::events::ScrapeRole;
use crate::domains::enrichment::events::{EnrichmentEvent, EnrichmentRole};
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::synthesis::events::{SynthesisEvent, SynthesisRole};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::core::stats::ScoutStats;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::SignalEvent;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::infra::util::sanitize_url;
use crate::core::extractor::ResourceTag;

/// Scheduling data passed between schedule_handler and scrape handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Clone, Serialize, Deserialize)]
pub struct PipelineState {
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

    /// Scrape roles completed in current phase, for phase-completion tracking.
    #[serde(default)]
    pub completed_scrape_roles: HashSet<ScrapeRole>,

    /// Synthesis roles completed, for phase-completion tracking.
    #[serde(default)]
    pub completed_synthesis_roles: HashSet<SynthesisRole>,

    /// Enrichment roles completed, for phase-completion tracking.
    #[serde(default)]
    pub completed_enrichment_roles: HashSet<EnrichmentRole>,

    /// Pipeline phases already completed — guards phase_complete handlers against duplicates.
    #[serde(default)]
    pub completed_phases: HashSet<PipelinePhase>,
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
    pub content_hash: String,
    pub resource_tags: Vec<ResourceTag>,
    pub signal_tags: Vec<String>,
    pub author_name: Option<String>,
    pub source_id: Option<Uuid>,
}

/// Edge-wiring data stashed by `create_signal_events` for `wire_signal_edges`.
/// Only the fields needed for wiring — the Node itself is already projected.
#[derive(Clone, Serialize, Deserialize)]
pub struct WiringContext {
    pub resource_tags: Vec<ResourceTag>,
    pub signal_tags: Vec<String>,
    pub author_name: Option<String>,
    pub source_id: Option<Uuid>,
}

impl PipelineState {
    pub fn new(url_to_canonical_key: HashMap<String, String>) -> Self {
        Self {
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
            completed_scrape_roles: HashSet::new(),
            completed_synthesis_roles: HashSet::new(),
            completed_enrichment_roles: HashSet::new(),
            completed_phases: HashSet::new(),
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
            ScrapeEvent::SourcesResolved { url_mappings, pub_dates, query_api_errors, .. } => {
                self.url_to_canonical_key.extend(url_mappings.clone());
                self.url_to_pub_date.extend(pub_dates.clone());
                self.query_api_errors.extend(query_api_errors.clone());
            }
            ScrapeEvent::ScrapeRoleCompleted {
                role,
                source_signal_counts,
                collected_links,
                expansion_queries,
                stats_delta,
                ..
            } => {
                self.completed_scrape_roles.insert(*role);
                for (k, v) in source_signal_counts {
                    *self.source_signal_counts.entry(k.clone()).or_default() += v;
                }
                self.collected_links.extend(collected_links.clone());
                self.expansion_queries.extend(expansion_queries.clone());
                self.stats.social_media_posts += stats_delta.social_media_posts;
                self.stats.discovery_posts_found += stats_delta.discovery_posts_found;
                self.stats.discovery_accounts_found += stats_delta.discovery_accounts_found;
                if *role == ScrapeRole::TopicDiscovery {
                    self.social_topics.clear();
                    self.social_expansion_topics.clear();
                }
            }
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
                *self.stats.by_type.entry(*node_type).or_default() += 1;
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
            DiscoveryEvent::SocialTopicsDiscovered { topics } => {
                self.social_topics = topics.clone();
            }
        }
    }

    /// Apply a synthesis domain event.
    pub fn apply_synthesis(&mut self, event: &SynthesisEvent) {
        match event {
            SynthesisEvent::SynthesisRoleCompleted { role, .. } => {
                self.completed_synthesis_roles.insert(*role);
            }
        }
    }

    /// Apply an expansion domain event.
    pub fn apply_expansion(&mut self, event: &ExpansionEvent) {
        match event {
            ExpansionEvent::ExpansionCompleted {
                social_expansion_topics,
                expansion_deferred_expanded,
                expansion_queries_collected,
                expansion_sources_created,
                expansion_social_topics_queued,
            } => {
                self.social_expansion_topics
                    .extend(social_expansion_topics.clone());
                self.stats.expansion_deferred_expanded = *expansion_deferred_expanded;
                self.stats.expansion_queries_collected = *expansion_queries_collected;
                self.stats.expansion_sources_created = *expansion_sources_created;
                self.stats.expansion_social_topics_queued = *expansion_social_topics_queued;
            }
        }
    }

    /// Apply an enrichment domain event.
    pub fn apply_enrichment(&mut self, event: &EnrichmentEvent) {
        match event {
            EnrichmentEvent::EnrichmentRoleCompleted { role } => {
                self.completed_enrichment_roles.insert(*role);
            }
        }
    }

    /// Apply a lifecycle domain event.
    pub fn apply_lifecycle(&mut self, event: &LifecycleEvent) {
        match event {
            LifecycleEvent::PhaseCompleted { phase } => {
                self.completed_phases.insert(phase.clone());
            }
            LifecycleEvent::SourcesScheduled {
                scheduled_data,
                actor_contexts,
                url_mappings,
                ..
            } => {
                self.actor_contexts.extend(actor_contexts.clone());
                self.url_to_canonical_key.extend(url_mappings.clone());
                self.scheduled = Some(scheduled_data.clone());
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------
    // Apply accumulated outputs from pure activity functions
    // -----------------------------------------------------------------

    /// Apply accumulated scrape output to pipeline state.
    pub fn apply_scrape_output(&mut self, output: crate::domains::scrape::activities::ScrapeOutput) {
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

    /// Apply a pipeline event to aggregate state.
    pub fn apply_pipeline(&mut self, event: &PipelineEvent) {
        match event {
            PipelineEvent::HandlerSkipped { .. } => {}
            PipelineEvent::HandlerFailed { .. } => {
                self.stats.handler_failures += 1;
            }
        }
    }
}

// ── Aggregate + Apply traits ─────────────────────────────────────

use seesaw_core::{Aggregate, aggregators};

impl Aggregate for PipelineState {
    fn aggregate_type() -> &'static str {
        "ScoutRun"
    }
}

#[aggregators(singleton)]
pub mod pipeline_aggregators {
    use super::*;

    fn on_signal(state: &mut PipelineState, event: SignalEvent) {
        state.apply_signal(&event);
    }

    fn on_scrape(state: &mut PipelineState, event: ScrapeEvent) {
        state.apply_scrape(&event);
    }

    fn on_discovery(state: &mut PipelineState, event: DiscoveryEvent) {
        state.apply_discovery(&event);
    }

    fn on_pipeline(state: &mut PipelineState, event: PipelineEvent) {
        state.apply_pipeline(&event);
    }

    fn on_synthesis(state: &mut PipelineState, event: SynthesisEvent) {
        state.apply_synthesis(&event);
    }

    fn on_enrichment(state: &mut PipelineState, event: EnrichmentEvent) {
        state.apply_enrichment(&event);
    }

    fn on_expansion(state: &mut PipelineState, event: ExpansionEvent) {
        state.apply_expansion(&event);
    }

    fn on_lifecycle(state: &mut PipelineState, event: LifecycleEvent) {
        state.apply_lifecycle(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler_failed(handler_id: &str, error: &str) -> PipelineEvent {
        PipelineEvent::HandlerFailed {
            handler_id: handler_id.to_string(),
            source_event_type: "ScrapeEvent".to_string(),
            error: error.to_string(),
            attempts: 3,
        }
    }

    #[test]
    fn handler_failure_increments_stats() {
        let mut state = PipelineState::default();
        assert_eq!(state.stats.handler_failures, 0);

        state.apply_pipeline(&handler_failed("scrape:fetch", "connection timeout"));
        assert_eq!(state.stats.handler_failures, 1);
    }

    #[test]
    fn multiple_handler_failures_accumulate() {
        let mut state = PipelineState::default();

        state.apply_pipeline(&handler_failed("scrape:fetch", "connection timeout"));
        state.apply_pipeline(&handler_failed("synthesis:linker", "LLM rate limit"));
        state.apply_pipeline(&handler_failed("enrichment:link_promoter", "DB error"));

        assert_eq!(state.stats.handler_failures, 3);
    }

    #[test]
    fn panic_in_handler_counted_as_failure() {
        let mut state = PipelineState::default();

        state.apply_pipeline(&PipelineEvent::HandlerFailed {
            handler_id: "scrape:extract".to_string(),
            source_event_type: "ScrapeEvent".to_string(),
            error: "panicked at 'index out of bounds: the len is 0 but the index is 5'".to_string(),
            attempts: 1,
        });

        assert_eq!(state.stats.handler_failures, 1);
    }

    #[test]
    fn handler_skipped_does_not_increment_failures() {
        let mut state = PipelineState::default();

        state.apply_pipeline(&PipelineEvent::HandlerSkipped {
            handler_id: "scrape:fetch".to_string(),
            reason: "no sources scheduled".to_string(),
        });

        assert_eq!(state.stats.handler_failures, 0);
    }
}

