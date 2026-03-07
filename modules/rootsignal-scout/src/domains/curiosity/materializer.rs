use anyhow::Result;
use chrono::Utc;
use seesaw_core::{Context, Events};
use tracing::info;
use uuid::Uuid;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::{
    ConcernNode, GatheringNode, GeoPoint, GeoPrecision, HelpRequestNode, Node, NodeMeta,
    ResourceOfferNode, ReviewStatus, ScoutScope, Severity, SensitivityLevel, Urgency,
};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::extractor::{ResourceRole, ResourceTag};
use crate::domains::curiosity::events::{CuriosityEvent, ResolvedEdge};
use crate::store::event_sourced::{node_system_events, node_to_world_event};

/// Materialize a discovery event into a WorldEvent + system events.
///
/// Fires during RECURSE phase — the reducer has already marked the discovered
/// entity's lifecycle (concern_linked, investigated) during REDUCE, preventing
/// curiosity handlers from re-processing the materialized signal.
pub async fn materialize(
    event: CuriosityEvent,
    ctx: &Context<ScoutEngineDeps>,
) -> Result<Events> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let region = state.run_scope.region();

    match event {
        CuriosityEvent::TensionDiscovered {
            tension_id,
            title,
            summary,
            severity,
            category: _,
            opposing,
            source_url,
            parent_signal_id,
            match_strength,
            explanation,
        } => {
            let severity = parse_severity(&severity);
            let meta = build_meta(tension_id, &title, &summary, &source_url, region, 0.7);

            let node = Node::Concern(ConcernNode {
                meta,
                severity,
                subject: None,
                opposing: Some(opposing),
            });

            let mut out = emit_node(&node);

            out.push(SystemEvent::ResponseLinked {
                signal_id: parent_signal_id,
                concern_id: tension_id,
                strength: match_strength.clamp(0.0, 1.0),
                explanation,
                source_url: None,
            });

            info!(tension_id = %tension_id, title = title.as_str(), "Materialized tension");
            Ok(out)
        }

        CuriosityEvent::SignalDiscovered {
            signal_id,
            title,
            summary,
            signal_type,
            url,
            parent_concern_id,
            match_strength,
            explanation,
            is_gravity,
            event_date,
            is_recurring,
            venue,
            organizer,
            gathering_type: _,
            what_needed,
            stated_goal,
            availability,
            eligibility,
            also_addresses,
            resources,
            diffusion_mechanism: _,
        } => {
            let meta = build_meta(signal_id, &title, &summary, &url, region, 0.7);

            let node = match signal_type.to_lowercase().as_str() {
                "gathering" => {
                    let starts_at = event_date.as_deref().and_then(|d| {
                        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                            .ok()
                            .map(|nd| nd.and_hms_opt(0, 0, 0).unwrap().and_utc())
                    });
                    Node::Gathering(GatheringNode {
                        meta,
                        starts_at,
                        ends_at: None,
                        action_url: url.clone(),
                        organizer,
                        is_recurring,
                    })
                }
                "help_request" => Node::HelpRequest(HelpRequestNode {
                    meta,
                    urgency: Urgency::Medium,
                    what_needed,
                    action_url: Some(url.clone()),
                    stated_goal,
                }),
                _ => Node::Resource(ResourceOfferNode {
                    meta,
                    action_url: url.clone(),
                    availability,
                    eligibility,
                    is_ongoing: is_recurring,
                }),
            };

            let mut out = emit_node(&node);

            emit_concern_edge(&mut out, signal_id, parent_concern_id, match_strength, &explanation, is_gravity);

            for edge in &also_addresses {
                emit_concern_edge(&mut out, signal_id, edge.concern_id, edge.similarity, "also addresses", is_gravity);
            }

            emit_resource_edges(&mut out, signal_id, &resources);

            if let Some(ref venue) = venue {
                emit_place_edges(&mut out, signal_id, venue, region);
            }

            info!(
                signal_id = %signal_id,
                title = title.as_str(),
                signal_type = signal_type.as_str(),
                is_gravity,
                "Materialized response"
            );
            Ok(out)
        }

        CuriosityEvent::EmergentTensionDiscovered {
            tension_id,
            title,
            summary,
            severity,
            opposing,
            source_url,
            parent_concern_id: _,
        } => {
            let severity = parse_severity(&severity);
            let meta = build_meta(tension_id, &title, &summary, &source_url, region, 0.4);

            let node = Node::Concern(ConcernNode {
                meta,
                severity,
                subject: None,
                opposing: Some(opposing),
            });

            let out = emit_node(&node);

            info!(tension_id = %tension_id, title = title.as_str(), "Materialized emergent tension (confidence 0.4)");
            Ok(out)
        }

        _ => Ok(Events::new()),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn emit_node(node: &Node) -> Events {
    let mut out = Events::new();
    out.push(node_to_world_event(node));
    for se in node_system_events(node) {
        out.push(se);
    }
    out
}

fn emit_concern_edge(
    out: &mut Events,
    signal_id: Uuid,
    concern_id: Uuid,
    strength: f64,
    explanation: &str,
    is_gravity: bool,
) {
    let strength = strength.clamp(0.0, 1.0);
    if is_gravity {
        out.push(SystemEvent::ConcernLinked {
            signal_id,
            concern_id,
            strength,
            explanation: explanation.to_string(),
            source_url: None,
        });
    } else {
        out.push(SystemEvent::ResponseLinked {
            signal_id,
            concern_id,
            strength,
            explanation: explanation.to_string(),
            source_url: None,
        });
    }
}

fn emit_resource_edges(out: &mut Events, signal_id: Uuid, resources: &[ResourceTag]) {
    for tag in resources.iter().filter(|t| t.confidence >= 0.3) {
        let slug = rootsignal_common::slugify(&tag.slug);
        let description = tag.context.as_deref().unwrap_or("");

        out.push(WorldEvent::ResourceIdentified {
            resource_id: Uuid::new_v4(),
            name: tag.slug.clone(),
            slug: slug.clone(),
            description: description.to_string(),
        });

        let confidence = tag.confidence.clamp(0.0, 1.0) as f32;
        let (quantity, capacity) = match tag.role {
            ResourceRole::Requires => (tag.context.clone(), None),
            ResourceRole::Prefers => (None, None),
            ResourceRole::Offers => (None, tag.context.clone()),
        };

        out.push(WorldEvent::ResourceLinked {
            signal_id,
            resource_slug: slug,
            role: tag.role.to_string(),
            confidence,
            quantity,
            notes: None,
            capacity,
        });
    }
}

fn emit_place_edges(
    out: &mut Events,
    signal_id: Uuid,
    venue: &str,
    region: Option<&ScoutScope>,
) {
    if venue.is_empty() {
        return;
    }

    let slug = rootsignal_common::slugify(venue);

    if let Some(r) = region {
        out.push(SystemEvent::PlaceDiscovered {
            place_id: Uuid::new_v4(),
            name: venue.to_string(),
            slug: slug.clone(),
            lat: r.center_lat,
            lng: r.center_lng,
            discovered_at: Utc::now(),
        });
    }

    out.push(SystemEvent::GathersAtPlaceLinked {
        signal_id,
        place_slug: slug,
    });
}

fn parse_severity(s: &str) -> Severity {
    match s.to_lowercase().as_str() {
        "low" => Severity::Low,
        "medium" => Severity::Medium,
        "high" => Severity::High,
        "critical" => Severity::Critical,
        _ => Severity::Medium,
    }
}

fn build_meta(
    id: Uuid,
    title: &str,
    summary: &str,
    source_url: &str,
    region: Option<&ScoutScope>,
    confidence: f32,
) -> NodeMeta {
    let now = Utc::now();
    NodeMeta {
        id,
        title: title.to_string(),
        summary: summary.to_string(),
        sensitivity: SensitivityLevel::General,
        confidence,
        corroboration_count: 0,
        about_location: region.map(|r| GeoPoint {
            lat: r.center_lat,
            lng: r.center_lng,
            precision: GeoPrecision::Approximate,
        }),
        from_location: None,
        about_location_name: region.map(|r| r.name.clone()),
        source_url: source_url.to_string(),
        extracted_at: now,
        published_at: None,
        last_confirmed_active: now,
        source_diversity: 1,
        cause_heat: 0.0,
        channel_diversity: 1,
        implied_queries: vec![],
        review_status: ReviewStatus::Staged,
        was_corrected: false,
        corrections: None,
        rejection_reason: None,
        mentioned_actors: Vec::new(),
        category: None,
    }
}
