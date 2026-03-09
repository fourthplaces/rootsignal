use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use rootsignal_common::{
    ConcernNode, GatheringNode, GeoPoint, GeoPrecision, HelpRequestNode, Location, Node,
    NodeMeta, ResourceOfferNode, ReviewStatus, ScoutScope, Severity, SensitivityLevel, Urgency,
};

use crate::core::extractor::{ResourceRole, ResourceTag};
use crate::domains::curiosity::events::{CuriosityEvent, ResolvedEdge};

// ---------------------------------------------------------------------------
// Domain types returned by the materializer
// ---------------------------------------------------------------------------

/// A materialized signal node with its graph edges.
pub struct MaterializedNode {
    pub node: Node,
    pub concern_edges: Vec<ConcernEdge>,
    pub resources: Vec<MaterializedResource>,
    pub venue: Option<VenueLink>,
}

/// An edge linking a signal to a concern (either responds-to or drawn-to).
pub struct ConcernEdge {
    pub signal_id: Uuid,
    pub concern_id: Uuid,
    pub strength: f64,
    pub explanation: String,
    pub is_gravity: bool,
}

/// A resource tag ready for event emission (pre-filtered, slug computed).
pub struct MaterializedResource {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub signal_id: Uuid,
    pub role: String,
    pub confidence: f32,
    pub quantity: Option<String>,
    pub capacity: Option<String>,
}

/// A venue linked to a gathering signal.
pub struct VenueLink {
    pub signal_id: Uuid,
    pub name: String,
    pub slug: String,
}

// ---------------------------------------------------------------------------
// Materializer: CuriosityEvent → domain types
// ---------------------------------------------------------------------------

/// Convert a discovery event into a materialized node with edges.
///
/// Returns None for non-discovery events (shouldn't happen due to handler filter).
pub fn materialize(
    event: CuriosityEvent,
    region: Option<&ScoutScope>,
) -> Option<MaterializedNode> {
    match event {
        CuriosityEvent::TensionDiscovered {
            tension_id,
            title,
            summary,
            severity,
            category: _,
            opposing,
            url,
            parent_signal_id,
            match_strength,
            explanation,
        } => {
            let severity = parse_severity(&severity);
            let meta = build_meta(tension_id, &title, &summary, &url, region, 0.7);

            let node = Node::Concern(ConcernNode {
                meta,
                severity,
                subject: None,
                opposing: Some(opposing),
            });

            let concern_edges = vec![ConcernEdge {
                signal_id: parent_signal_id,
                concern_id: tension_id,
                strength: match_strength.clamp(0.0, 1.0),
                explanation,
                is_gravity: false,
            }];

            info!(tension_id = %tension_id, title = title.as_str(), "Materialized tension");

            Some(MaterializedNode {
                node,
                concern_edges,
                resources: vec![],
                venue: None,
            })
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

            let mut concern_edges = vec![ConcernEdge {
                signal_id,
                concern_id: parent_concern_id,
                strength: match_strength.clamp(0.0, 1.0),
                explanation,
                is_gravity,
            }];

            for edge in &also_addresses {
                concern_edges.push(ConcernEdge {
                    signal_id,
                    concern_id: edge.concern_id,
                    strength: edge.similarity.clamp(0.0, 1.0),
                    explanation: "also addresses".to_string(),
                    is_gravity,
                });
            }

            let materialized_resources = materialize_resources(signal_id, &resources);

            let venue_link = venue.as_deref()
                .filter(|v| !v.is_empty())
                .map(|v| VenueLink {
                    signal_id,
                    name: v.to_string(),
                    slug: rootsignal_common::slugify(v),
                });

            info!(
                signal_id = %signal_id,
                title = title.as_str(),
                signal_type = signal_type.as_str(),
                is_gravity,
                "Materialized response"
            );

            Some(MaterializedNode {
                node,
                concern_edges,
                resources: materialized_resources,
                venue: venue_link,
            })
        }

        CuriosityEvent::EmergentTensionDiscovered {
            tension_id,
            title,
            summary,
            severity,
            opposing,
            url,
            parent_concern_id: _,
        } => {
            let severity = parse_severity(&severity);
            let meta = build_meta(tension_id, &title, &summary, &url, region, 0.4);

            let node = Node::Concern(ConcernNode {
                meta,
                severity,
                subject: None,
                opposing: Some(opposing),
            });

            info!(tension_id = %tension_id, title = title.as_str(), "Materialized emergent tension (confidence 0.4)");

            Some(MaterializedNode {
                node,
                concern_edges: vec![],
                resources: vec![],
                venue: None,
            })
        }

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn materialize_resources(signal_id: Uuid, resources: &[ResourceTag]) -> Vec<MaterializedResource> {
    resources
        .iter()
        .filter(|t| t.confidence >= 0.3)
        .map(|tag| {
            let slug = rootsignal_common::slugify(&tag.slug);
            let description = tag.context.as_deref().unwrap_or("").to_string();
            let (quantity, capacity) = match tag.role {
                ResourceRole::Requires => (tag.context.clone(), None),
                ResourceRole::Prefers => (None, None),
                ResourceRole::Offers => (None, tag.context.clone()),
            };

            MaterializedResource {
                name: tag.slug.clone(),
                slug,
                description,
                signal_id,
                role: tag.role.to_string(),
                confidence: tag.confidence.clamp(0.0, 1.0) as f32,
                quantity,
                capacity,
            }
        })
        .collect()
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
        locations: region.map(|r| vec![Location {
            point: Some(GeoPoint {
                lat: r.center_lat,
                lng: r.center_lng,
                precision: GeoPrecision::Approximate,
            }),
            name: Some(r.name.clone()),
            address: None,
            role: None,
        }]).unwrap_or_default(),
        url: source_url.to_string(),
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
        mentioned_entities: vec![],
        category: None,
    }
}
