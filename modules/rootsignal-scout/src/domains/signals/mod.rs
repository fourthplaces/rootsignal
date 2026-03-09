pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_common::events::{SystemEvent, WorldEvent};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::activities::dedup;
use crate::domains::signals::events::{ActorAction, DedupOutcome, NewCitation, SignalEvent};
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

    /// Scrape completed → run dedup on all extracted batches.
    #[handle(on = ScrapeEvent, id = "signals:dedup_signals", filter = is_scrape_completed)]
    async fn dedup_signals(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let extracted_batches = event.into_extracted_batches();

        if extracted_batches.is_empty() {
            ctx.logger.debug("No extracted batches, emitting NoNewSignals");
            return Ok(events![SignalEvent::NoNewSignals]);
        }

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let mut all_events = Events::new();
        let mut has_created = false;
        let mut total_created = 0u32;
        let mut total_refreshed = 0u32;

        for extraction in &extracted_batches {
            let result = dedup::deduplicate_extracted_batch(
                &extraction.url,
                &extraction.canonical_key,
                &extraction.batch,
                &state,
                deps,
            ).await?;

            total_created += result.created.len() as u32;
            total_refreshed += result.verdicts.iter()
                .filter(|v| matches!(v, crate::domains::signals::events::DedupOutcome::Refreshed { .. }))
                .count() as u32;
            has_created = has_created || !result.created.is_empty();

            // Created signals: world fact + system classifications + citation
            for signal in result.created {
                let schedule = extraction.batch.schedules.get(&signal.node.id());
                all_events.push(node_to_world_event(&signal.node, schedule));
                for sys in node_system_events(&signal.node) {
                    all_events.push(sys);
                }
                all_events.push(citation_published(signal.citation));
            }

            // Content-changed signals: updated world fact + new citation
            for changed in result.content_changed {
                all_events.push(WorldEvent::DetailsChanged {
                    signal_id: changed.existing_id,
                    node_type: changed.node.node_type(),
                    title: changed.node.title().to_string(),
                    summary: changed.node.meta().map(|m| m.summary.clone()).unwrap_or_default(),
                    url: changed.citation.url.clone(),
                });
                all_events.push(citation_published(changed.citation));
            }

            // Actor actions from inline resolution
            for action in result.actor_actions {
                match action {
                    ActorAction::Identified { actor_id, name, canonical_key, actor_type } => {
                        all_events.push(SystemEvent::ActorIdentified {
                            actor_id,
                            name,
                            actor_type,
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

            // Wiring events: emit projectable facts for data that was
            // previously written directly to Neo4j by project_dedup_verdicts().
            // These flow through the projector's existing arms on replay.
            let mut refreshed_ids: Vec<(uuid::Uuid, rootsignal_common::types::NodeType)> = Vec::new();
            for verdict in &result.verdicts {
                match verdict {
                    DedupOutcome::Created {
                        node_id,
                        source_id,
                        resource_tags,
                        signal_tags,
                        ..
                    } => {
                        if let Some(sid) = source_id {
                            all_events.push(WorldEvent::SignalLinkedToSource {
                                signal_id: *node_id,
                                source_id: *sid,
                            });
                        }

                        for tag in resource_tags.iter().filter(|t| t.confidence >= 0.3) {
                            let slug = rootsignal_common::slugify(&tag.slug);
                            all_events.push(WorldEvent::ResourceIdentified {
                                resource_id: uuid::Uuid::new_v4(),
                                name: tag.slug.clone(),
                                slug: slug.clone(),
                                description: tag.context.clone().unwrap_or_default(),
                            });
                            let (quantity, capacity) = match tag.role {
                                crate::core::extractor::ResourceRole::Requires => {
                                    (tag.context.clone(), None)
                                }
                                crate::core::extractor::ResourceRole::Prefers => (None, None),
                                crate::core::extractor::ResourceRole::Offers => {
                                    (None, tag.context.clone())
                                }
                            };
                            all_events.push(WorldEvent::ResourceLinked {
                                signal_id: *node_id,
                                resource_slug: slug,
                                role: tag.role.to_string(),
                                confidence: tag.confidence.clamp(0.0, 1.0) as f32,
                                quantity,
                                notes: None,
                                capacity,
                            });
                        }

                        if !signal_tags.is_empty() {
                            all_events.push(SystemEvent::SignalTagged {
                                signal_id: *node_id,
                                tag_slugs: signal_tags.clone(),
                            });
                        }
                    }
                    DedupOutcome::Refreshed { existing_id, node_type, .. }
                    | DedupOutcome::ContentChanged { existing_id, node_type, .. } => {
                        refreshed_ids.push((*existing_id, *node_type));
                    }
                }
            }

            // Group refreshed signals by node_type for FreshnessConfirmed batches
            if !refreshed_ids.is_empty() {
                use std::collections::HashMap;
                let mut by_type: HashMap<rootsignal_common::types::NodeType, Vec<uuid::Uuid>> =
                    HashMap::new();
                for (id, nt) in refreshed_ids {
                    by_type.entry(nt).or_default().push(id);
                }
                let now = chrono::Utc::now();
                for (node_type, signal_ids) in by_type {
                    all_events.push(SystemEvent::FreshnessConfirmed {
                        signal_ids,
                        node_type,
                        confirmed_at: now,
                    });
                }
            }

            all_events.push(SignalEvent::DedupCompleted {
                url: extraction.url.clone(),
                canonical_key: extraction.canonical_key.clone(),
                verdicts: result.verdicts,
            });
        }

        ctx.logger.info(&format!(
            "Dedup: {} created, {} refreshed from {} batches",
            total_created, total_refreshed, extracted_batches.len(),
        ));

        if !has_created {
            all_events.push(SignalEvent::NoNewSignals);
        }

        Ok(all_events)
    }
}
