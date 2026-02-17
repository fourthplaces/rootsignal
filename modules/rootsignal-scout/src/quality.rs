use rootsignal_common::{ExtractionQuality, GeoAccuracy, Node};

/// Compute extraction quality for a signal node.
pub fn score(node: &Node) -> ExtractionQuality {
    let meta = match node.meta() {
        Some(m) => m,
        None => {
            return ExtractionQuality {
                actionable: false,
                has_location: false,
                has_action_url: false,
                has_timing: false,

                geo_accuracy: GeoAccuracy::Low,
                completeness: 0.0,
                confidence: 0.0,
            };
        }
    };

    let has_location = meta.location.is_some();
    let geo_accuracy = match meta.location {
        Some(ref loc) => match loc.precision {
            rootsignal_common::GeoPrecision::Exact => GeoAccuracy::High,
            rootsignal_common::GeoPrecision::Neighborhood => GeoAccuracy::Medium,
            _ => GeoAccuracy::Low,
        },
        None => GeoAccuracy::Low,
    };

    let (has_action_url, has_timing) = match node {
        Node::Event(e) => {
            let has_real_url = !e.action_url.is_empty() && e.action_url != meta.source_url;
            let has_real_timing = e.starts_at.is_some();
            (has_real_url, has_real_timing)
        }
        Node::Give(g) => (!g.action_url.is_empty(), g.is_ongoing),
        Node::Ask(a) => (a.action_url.is_some(), false),
        Node::Notice(_) => (false, false),
        Node::Tension(_) => (false, false),
        Node::Evidence(_) => (false, false),
    };

    let actionable = matches!(node, Node::Notice(_))
        || (has_action_url
            && (has_timing
                || matches!(node, Node::Give(g) if g.is_ongoing)
                || matches!(node, Node::Ask(_))));

    // Completeness: fraction of 7 key fields populated
    // title, summary, signal_type (always), audience_roles, location, action_url, timing
    let mut filled = 3.0_f32; // title, summary, signal_type always present
    if !meta.audience_roles.is_empty() {
        filled += 1.0;
    }
    if has_location {
        filled += 1.0;
    }
    if has_action_url {
        filled += 1.0;
    }
    if has_timing {
        filled += 1.0;
    }
    let completeness = filled / 7.0;

    // Confidence: weighted average
    let geo_score = match geo_accuracy {
        GeoAccuracy::High => 1.0_f32,
        GeoAccuracy::Medium => 0.7,
        GeoAccuracy::Low => 0.3,
    };
    let confidence =
        completeness * 0.4 + geo_score * 0.3 + meta.freshness_score * 0.3;

    ExtractionQuality {
        actionable,
        has_location,
        has_action_url,
        has_timing,
        geo_accuracy,
        completeness,
        confidence: confidence.clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rootsignal_common::{
        AudienceRole, EventNode, GeoPoint, GeoPrecision, NodeMeta, SensitivityLevel,
    };
    use uuid::Uuid;

    fn test_meta() -> NodeMeta {
        NodeMeta {
            id: Uuid::new_v4(),
            title: "Community Dinner".to_string(),
            summary: "Free dinner at the park".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.0,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: Some(GeoPoint {
                lat: 44.97,
                lng: -93.26,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("Powderhorn Park".to_string()),
            source_url: "https://example.com/events".to_string(),
            extracted_at: Utc::now(),
            last_confirmed_active: Utc::now(),
            audience_roles: vec![AudienceRole::Neighbor],
            mentioned_actors: vec![],
        }
    }

    #[test]
    fn event_with_real_date_scores_higher_than_without() {
        let meta = test_meta();

        let with_date = Node::Event(EventNode {
            meta: meta.clone(),
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: "https://example.com/rsvp".to_string(),
            organizer: None,
            is_recurring: false,
        });

        let without_date = Node::Event(EventNode {
            meta: meta.clone(),
            starts_at: None,
            ends_at: None,
            action_url: "https://example.com/rsvp".to_string(),
            organizer: None,
            is_recurring: false,
        });

        let q_with = score(&with_date);
        let q_without = score(&without_date);

        assert!(q_with.has_timing);
        assert!(!q_without.has_timing);
        assert!(q_with.completeness > q_without.completeness);
        assert!(q_with.confidence > q_without.confidence);
    }

    #[test]
    fn event_with_date_is_actionable() {
        let meta = test_meta();
        let event = Node::Event(EventNode {
            meta,
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: "https://example.com/rsvp".to_string(),
            organizer: None,
            is_recurring: false,
        });

        let q = score(&event);
        assert!(q.actionable);
    }

    #[test]
    fn event_without_date_or_url_is_not_actionable() {
        let mut meta = test_meta();
        meta.source_url = "https://example.com".to_string();
        let event = Node::Event(EventNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: "https://example.com".to_string(), // same as source_url
            organizer: None,
            is_recurring: false,
        });

        let q = score(&event);
        assert!(!q.actionable);
    }
}
