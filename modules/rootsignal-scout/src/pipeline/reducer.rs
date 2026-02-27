//! Pure state updates for the scout pipeline.
//!
//! The reducer sees every event and updates `PipelineState` accordingly.
//! No I/O, no side effects — just bookkeeping.

use rootsignal_engine::Reducer;

use crate::pipeline::events::{FreshnessBucket, PipelineEvent, ScoutEvent};
use crate::pipeline::state::{PipelineState, WiringContext};

use crate::enrichment::link_promoter::CollectedLink;

pub struct ScoutReducer;

impl Reducer<ScoutEvent, PipelineState> for ScoutReducer {
    fn reduce(&self, state: &mut PipelineState, event: &ScoutEvent) {
        let ScoutEvent::Pipeline(pe) = event else {
            // World and System events don't update pipeline state.
            return;
        };

        match pe {
            // Content fetching
            PipelineEvent::ContentFetched { .. } => {
                state.stats.urls_scraped += 1;
            }
            PipelineEvent::ContentUnchanged { .. } => {
                state.stats.urls_unchanged += 1;
            }
            PipelineEvent::ContentFetchFailed { .. } => {
                state.stats.urls_failed += 1;
            }

            // Extraction
            PipelineEvent::SignalsExtracted { count, .. } => {
                state.stats.signals_extracted += count;
            }

            // Dedup verdicts
            PipelineEvent::NewSignalAccepted {
                node_id,
                node_type,
                pending_node,
                ..
            } => {
                state.stats.signals_stored += 1;
                if let Some(idx) = signal_type_index(node_type) {
                    state.stats.by_type[idx] += 1;
                }
                // Stash WiringContext for handle_signal_stored
                state.wiring_contexts.insert(
                    *node_id,
                    WiringContext {
                        resource_tags: pending_node.resource_tags.clone(),
                        signal_tags: pending_node.signal_tags.clone(),
                        author_name: pending_node.author_name.clone(),
                        source_id: pending_node.source_id,
                    },
                );
                // Stash PendingNode for handle_create
                state.pending_nodes.insert(*node_id, *pending_node.clone());
            }
            PipelineEvent::CrossSourceMatchDetected { .. }
            | PipelineEvent::SameSourceReencountered { .. } => {
                state.stats.signals_deduplicated += 1;
            }

            // URL-level summary
            PipelineEvent::UrlProcessed {
                canonical_key,
                signals_created,
                ..
            } => {
                *state
                    .source_signal_counts
                    .entry(canonical_key.clone())
                    .or_default() += signals_created;
            }

            // Links
            PipelineEvent::LinkCollected {
                url,
                discovered_on,
            } => {
                state.collected_links.push(CollectedLink {
                    url: url.clone(),
                    discovered_on: discovered_on.clone(),
                });
            }

            // Expansion
            PipelineEvent::ExpansionQueryCollected { query, .. } => {
                state.expansion_queries.push(query.clone());
                state.stats.expansion_queries_collected += 1;
            }
            PipelineEvent::SocialTopicCollected { topic } => {
                state.social_expansion_topics.push(topic.clone());
                state.stats.expansion_social_topics_queued += 1;
            }

            // Social
            PipelineEvent::SocialPostsFetched { count, .. } => {
                state.stats.social_media_posts += count;
            }

            // Freshness
            PipelineEvent::FreshnessRecorded { bucket, .. } => match bucket {
                FreshnessBucket::Within7d => state.stats.fresh_7d += 1,
                FreshnessBucket::Within30d => state.stats.fresh_30d += 1,
                FreshnessBucket::Within90d => state.stats.fresh_90d += 1,
                FreshnessBucket::Older | FreshnessBucket::Unknown => {}
            },

            // SignalReaderd — clean up PendingNode (WiringContext stays until end of run)
            PipelineEvent::SignalReaderd { node_id, .. } => {
                state.pending_nodes.remove(node_id);
            }

            // DedupCompleted — clean up extracted batch
            PipelineEvent::DedupCompleted { url } => {
                state.extracted_batches.remove(url);
            }

            // Phase lifecycle / engine lifecycle — no state changes
            PipelineEvent::PhaseStarted { .. }
            | PipelineEvent::PhaseCompleted { .. }
            | PipelineEvent::ExtractionFailed { .. }
            | PipelineEvent::EngineStarted { .. } => {}

            PipelineEvent::SourceDiscovered { .. } => {
                state.stats.sources_discovered += 1;
            }
        }
    }
}

use rootsignal_common::types::NodeType;

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
