//! Pipeline state managed by the aggregate + handler stash.
//!
//! `PipelineState` is the mutable state for a scout run. State mutations
//! happen in per-domain `apply_*` methods (pure, synchronous), not
//! scattered across handlers.
//!

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use rootsignal_common::types::{ActorContext, NodeType, SourceNode};
use rootsignal_common::{scraping_strategy, ScrapingStrategy};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_common::Node;

use crate::core::pipeline_events::PipelineEvent;
use crate::core::run_scope::RunScope;
use crate::domains::scrape::events::ScrapeRole;
use crate::domains::enrichment::events::{EnrichmentEvent, EnrichmentRole};
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::synthesis::events::{SynthesisEvent, SynthesisRole};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::core::stats::ScoutStats;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::SignalEvent;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::supervisor::events::SupervisorEvent;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::infra::util::sanitize_url;
use crate::core::extractor::ResourceTag;

/// Source plan for this run: which sources to process and how they're partitioned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePlan {
    pub all_sources: Vec<SourceNode>,
    pub selected_sources: Vec<SourceNode>,
    pub tension_phase_keys: HashSet<String>,
    pub response_phase_keys: HashSet<String>,
    pub selected_keys: HashSet<String>,
    pub consumed_pin_ids: Vec<Uuid>,
}

/// Output from source preparation: the plan plus context maps.
pub struct SourcePlanOutput {
    pub source_plan: SourcePlan,
    pub actor_contexts: HashMap<String, ActorContext>,
    pub url_mappings: HashMap<String, String>,
    pub tension_count: u32,
    pub response_count: u32,
}

/// Mutable state for a scout run, updated by the reducer.
#[derive(Clone, Serialize, Deserialize)]
pub struct PipelineState {
    /// Run scope: geographic/source context for this run.
    #[serde(default)]
    pub run_scope: RunScope,

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

    /// Content previews (first 500 chars) keyed by URL, for page triage in link promotion.
    #[serde(default)]
    pub page_previews: HashMap<String, String>,

    /// Edge-wiring context stashed by reducer on `SignalCreated` for `wire_signal_edges`.
    pub wiring_contexts: HashMap<Uuid, WiringContext>,

    /// Source plan stashed by prepare_sources, consumed by scrape handlers.
    pub source_plan: Option<SourcePlan>,

    /// Social topics collected during mid-run discovery, consumed by response scrape.
    pub social_topics: Vec<String>,

    /// Scrape roles expected for each phase, derived from the source plan.
    #[serde(default)]
    pub expected_tension_roles: HashSet<ScrapeRole>,
    #[serde(default)]
    pub expected_response_roles: HashSet<ScrapeRole>,

    /// Scrape roles completed in current phase, for phase-completion tracking.
    #[serde(default)]
    pub completed_scrape_roles: HashSet<ScrapeRole>,

    /// Synthesis roles completed, for phase-completion tracking.
    #[serde(default)]
    pub completed_synthesis_roles: HashSet<SynthesisRole>,

    /// Enrichment roles completed, for phase-completion tracking.
    #[serde(default)]
    pub completed_enrichment_roles: HashSet<EnrichmentRole>,

    /// Whether source expansion has completed (guards against re-triggering).
    #[serde(default)]
    pub source_expansion_completed: bool,
}

/// A batch of extracted nodes for a single URL, carried on scrape completion events
/// as in-memory data for the dedup handler.
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiringContext {
    pub resource_tags: Vec<ResourceTag>,
    pub signal_tags: Vec<String>,
    pub author_name: Option<String>,
    pub source_id: Option<Uuid>,
}

impl PipelineState {
    pub fn new(url_to_canonical_key: HashMap<String, String>) -> Self {
        Self {
            run_scope: RunScope::default(),
            url_to_canonical_key,
            source_signal_counts: HashMap::new(),
            expansion_queries: Vec::new(),
            social_expansion_topics: Vec::new(),
            stats: ScoutStats::default(),
            query_api_errors: HashSet::new(),
            actor_contexts: HashMap::new(),
            url_to_pub_date: HashMap::new(),
            collected_links: Vec::new(),
            page_previews: HashMap::new(),
            wiring_contexts: HashMap::new(),
            source_plan: None,
            social_topics: Vec::new(),
            expected_tension_roles: HashSet::new(),
            expected_response_roles: HashSet::new(),
            completed_scrape_roles: HashSet::new(),
            completed_synthesis_roles: HashSet::new(),
            completed_enrichment_roles: HashSet::new(),
            source_expansion_completed: false,
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
            ScrapeEvent::SourcesResolved { url_mappings, pub_dates, query_api_errors, .. } => {
                self.url_to_canonical_key.extend(url_mappings.clone());
                self.url_to_pub_date.extend(pub_dates.clone());
                self.query_api_errors.extend(query_api_errors.clone());
            }
            ScrapeEvent::WebScrapeCompleted {
                role,
                urls_scraped,
                urls_unchanged,
                urls_failed,
                signals_extracted,
                source_signal_counts,
                collected_links,
                expansion_queries,
                page_previews,
                ..
            } => {
                self.completed_scrape_roles.insert(*role);
                self.stats.urls_scraped += urls_scraped;
                self.stats.urls_unchanged += urls_unchanged;
                self.stats.urls_failed += urls_failed;
                self.stats.signals_extracted += signals_extracted;
                for (k, v) in source_signal_counts {
                    *self.source_signal_counts.entry(k.clone()).or_default() += v;
                }
                self.collected_links.extend(collected_links.clone());
                self.expansion_queries.extend(expansion_queries.clone());
                self.page_previews.extend(page_previews.clone());
            }
            ScrapeEvent::SocialScrapeCompleted {
                role,
                signals_extracted,
                source_signal_counts,
                collected_links,
                expansion_queries,
                stats_delta,
                sources_scraped,
                ..
            } => {
                self.completed_scrape_roles.insert(*role);
                self.stats.urls_scraped += sources_scraped;
                self.stats.signals_extracted += signals_extracted;
                for (k, v) in source_signal_counts {
                    *self.source_signal_counts.entry(k.clone()).or_default() += v;
                }
                self.collected_links.extend(collected_links.clone());
                self.expansion_queries.extend(expansion_queries.clone());
                self.stats.social_media_posts += stats_delta.social_media_posts;
                self.stats.discovery_posts_found += stats_delta.discovery_posts_found;
                self.stats.discovery_accounts_found += stats_delta.discovery_accounts_found;
            }
            ScrapeEvent::TopicDiscoveryCompleted {
                source_signal_counts,
                collected_links,
                expansion_queries,
                stats_delta,
                ..
            } => {
                self.completed_scrape_roles.insert(ScrapeRole::TopicDiscovery);
                for (k, v) in source_signal_counts {
                    *self.source_signal_counts.entry(k.clone()).or_default() += v;
                }
                self.collected_links.extend(collected_links.clone());
                self.expansion_queries.extend(expansion_queries.clone());
                self.stats.social_media_posts += stats_delta.social_media_posts;
                self.stats.discovery_posts_found += stats_delta.discovery_posts_found;
                self.stats.discovery_accounts_found += stats_delta.discovery_accounts_found;
                self.social_topics.clear();
                self.social_expansion_topics.clear();
            }
            ScrapeEvent::ResponseScrapeSkipped { .. } => {
                for role in &self.expected_response_roles.clone() {
                    self.completed_scrape_roles.insert(*role);
                }
                // No graph/region = no enrichment work possible — mark all roles done
                // so enrichment handlers reject via filter and update_source_weights
                // fires directly on this event.
                for role in crate::domains::enrichment::events::all_enrichment_roles() {
                    self.completed_enrichment_roles.insert(role);
                }
            }
        }
    }

    /// Apply a signal domain event.
    pub fn apply_signal(&mut self, event: &SignalEvent) {
        match event {
            SignalEvent::SignalCreated { node_id, node_type, wiring, .. } => {
                self.stats.signals_stored += 1;
                *self.stats.by_type.entry(*node_type).or_default() += 1;
                if let Some(w) = wiring {
                    self.wiring_contexts.insert(*node_id, w.clone());
                }
            }
            SignalEvent::DedupCompleted {
                canonical_key,
                signals_created,
                signals_deduplicated,
                ..
            } => {
                *self
                    .source_signal_counts
                    .entry(canonical_key.clone())
                    .or_default() += signals_created;
                self.stats.signals_deduplicated += *signals_deduplicated;
            }
        }
    }

    /// Apply a discovery domain event.
    pub fn apply_discovery(&mut self, event: &DiscoveryEvent) {
        match event {
            DiscoveryEvent::SourcesDiscovered { sources, discovered_by } => {
                self.stats.sources_discovered += sources.len() as u32;
                if discovered_by == "link_promoter" {
                    self.collected_links.clear();
                    self.page_previews.clear();
                }
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
            DiscoveryEvent::SourceExpansionCompleted => {
                self.source_expansion_completed = true;
            }
            DiscoveryEvent::SourceExpansionSkipped { .. } => {
                self.source_expansion_completed = true;
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
            LifecycleEvent::ScoutRunRequested { scope, .. } => {
                self.run_scope = scope.clone();
            }
            LifecycleEvent::SourcesPrepared {
                source_plan,
                actor_contexts,
                url_mappings,
                pub_dates,
                query_api_errors,
                ..
            } => {
                self.actor_contexts.extend(actor_contexts.clone());
                self.url_to_canonical_key.extend(url_mappings.clone());
                self.url_to_pub_date.extend(pub_dates.clone());
                self.query_api_errors.extend(query_api_errors.clone());

                // Derive expected roles from what's actually in the plan
                let has_tension_social = source_plan.selected_sources.iter().any(|s| {
                    matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                        && source_plan.tension_phase_keys.contains(&s.canonical_key)
                });
                let has_response_social = source_plan.selected_sources.iter().any(|s| {
                    matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                        && source_plan.response_phase_keys.contains(&s.canonical_key)
                });

                self.expected_tension_roles = {
                    let mut roles = HashSet::from([ScrapeRole::TensionWeb]);
                    if has_tension_social { roles.insert(ScrapeRole::TensionSocial); }
                    roles
                };
                self.expected_response_roles = {
                    let mut roles = HashSet::from([ScrapeRole::ResponseWeb, ScrapeRole::TopicDiscovery]);
                    if has_response_social { roles.insert(ScrapeRole::ResponseSocial); }
                    roles
                };

                self.source_plan = Some(source_plan.clone());
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

    fn on_situation_weaving(_state: &mut PipelineState, _event: SituationWeavingEvent) {}

    fn on_supervisor(_state: &mut PipelineState, _event: SupervisorEvent) {}
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

}

