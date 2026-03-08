pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_common::events::{SystemEvent, WorldEvent};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::activities::dedup;
use crate::domains::signals::events::{ActorAction, NewCitation, SignalEvent};
use crate::domains::scrape::events::ScrapeEvent;
use crate::store::event_sourced::{node_system_events, node_to_world_event};

fn is_scrape_completed(e: &ScrapeEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.is_completion()
}

fn citation_published(c: NewCitation) -> WorldEvent {
    WorldEvent::CitationPublished {
        citation_id: c.citation_id,
        signal_id: c.signal_id,
        url: c.url,
        content_hash: c.content_hash,
        snippet: c.snippet,
        relevance: None,
        channel_type: c.channel_type,
        evidence_confidence: None,
    }
}

#[handlers]
pub mod handlers {
    use super::*;

    /// Scrape completed → run 4-layer dedup on all extracted batches.
    #[handle(on = ScrapeEvent, id = "signals:dedup_signals", filter = is_scrape_completed)]
    async fn dedup_signals(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let extracted_batches = event.into_extracted_batches();

        if extracted_batches.is_empty() {
            return Ok(events![SignalEvent::NoNewSignals]);
        }

        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let mut all_events = Events::new();
        let mut has_created = false;

        for extraction in &extracted_batches {
            let result = dedup::deduplicate_extracted_batch(
                &extraction.url,
                &extraction.canonical_key,
                &extraction.batch,
                &state,
                deps,
            ).await?;

            has_created = has_created || !result.created.is_empty();

            // Created signals: world fact + system classifications + citation
            for signal in result.created {
                all_events.push(node_to_world_event(&signal.node));
                for sys in node_system_events(&signal.node) {
                    all_events.push(sys);
                }
                all_events.push(citation_published(signal.citation));
            }

            // Corroborations: citation + observation + score
            for corr in result.corroborations {
                all_events.push(citation_published(corr.citation));
                all_events.push(SystemEvent::ObservationCorroborated {
                    signal_id: corr.signal_id,
                    node_type: corr.node_type,
                    new_url: corr.url,
                    summary: None,
                });
                all_events.push(SystemEvent::CorroborationScored {
                    signal_id: corr.signal_id,
                    similarity: corr.similarity,
                    new_corroboration_count: corr.new_corroboration_count,
                });
            }

            // Actor actions from inline resolution
            for action in result.actor_actions {
                match action {
                    ActorAction::Identified { actor_id, name, canonical_key } => {
                        all_events.push(SystemEvent::ActorIdentified {
                            actor_id,
                            name,
                            actor_type: rootsignal_common::ActorType::Organization,
                            canonical_key,
                            domains: vec![],
                            social_urls: vec![],
                            description: String::new(),
                            bio: None,
                            location_lat: None,
                            location_lng: None,
                            location_name: None,
                        });
                    }
                    ActorAction::LinkedToSource { actor_id, source_id } => {
                        all_events.push(WorldEvent::ActorLinkedToSource {
                            actor_id,
                            source_id,
                        });
                    }
                    ActorAction::LinkedToSignal { actor_id, signal_id } => {
                        all_events.push(SystemEvent::ActorLinkedToSignal {
                            actor_id,
                            signal_id,
                            role: "authored".to_string(),
                        });
                    }
                }
            }

            all_events.push(SignalEvent::DedupCompleted {
                url: extraction.url.clone(),
                canonical_key: extraction.canonical_key.clone(),
                verdicts: result.verdicts,
            });
        }

        if !has_created {
            all_events.push(SignalEvent::NoNewSignals);
        }

        Ok(all_events)
    }
}
