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
        Node::Gathering(e) => {
            let has_real_url = !e.action_url.is_empty() && e.action_url != meta.source_url;
            let has_real_timing = e.starts_at.is_some();
            (has_real_url, has_real_timing)
        }
        Node::Aid(g) => (!g.action_url.is_empty(), g.is_ongoing),
        Node::Need(a) => (a.action_url.is_some(), false),
        Node::Notice(_) => (false, false),
        Node::Tension(_) => (false, false),
        Node::Evidence(_) => (false, false),
    };

    let actionable = matches!(node, Node::Notice(_))
        || (has_action_url
            && (has_timing
                || matches!(node, Node::Aid(g) if g.is_ongoing)
                || matches!(node, Node::Need(_))));

    // Completeness: fraction of *applicable* optional fields populated.
    // Always-present fields (title, summary, signal_type) are prerequisites, not differentiators.
    // Denominator varies by node type so Notice/Tension aren't penalized for fields they can't have.
    let (optional_filled, optional_total) = match node {
        Node::Gathering(_) => {
            // location, action_url, timing
            let filled = has_location as u8 + has_action_url as u8 + has_timing as u8;
            (filled, 3u8)
        }
        Node::Aid(_) => {
            // location, action_url, is_ongoing
            let filled = has_location as u8 + has_action_url as u8 + has_timing as u8;
            (filled, 3)
        }
        Node::Need(_) => {
            // location, action_url
            let filled = has_location as u8 + has_action_url as u8;
            (filled, 2)
        }
        Node::Notice(_) | Node::Tension(_) => {
            // location only
            (has_location as u8, 1)
        }
        Node::Evidence(_) => (0, 0),
    };
    let completeness = if optional_total > 0 {
        optional_filled as f32 / optional_total as f32
    } else {
        0.0
    };

    // Confidence: completeness and geo quality, equally weighted.
    // Freshness is always 1.0 at extraction time (no discrimination), so it's excluded here.
    // Apply freshness as a time-based decay multiplier separately if needed.
    let geo_score = match geo_accuracy {
        GeoAccuracy::High => 1.0_f32,
        GeoAccuracy::Medium => 0.7,
        GeoAccuracy::Low => 0.3,
    };
    let confidence = completeness * 0.5 + geo_score * 0.5;

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
    use rootsignal_common::{GatheringNode, GeoPoint, GeoPrecision, NodeMeta, SensitivityLevel};
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
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            mentioned_actors: vec![],
            implied_queries: vec![],
        }
    }

    #[test]
    fn gathering_with_real_date_scores_higher_than_without() {
        let meta = test_meta();

        let with_date = Node::Gathering(GatheringNode {
            meta: meta.clone(),
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: "https://example.com/rsvp".to_string(),
            organizer: None,
            is_recurring: false,
        });

        let without_date = Node::Gathering(GatheringNode {
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
    fn gathering_with_date_is_actionable() {
        let meta = test_meta();
        let event = Node::Gathering(GatheringNode {
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
    fn bare_gathering_scores_low_confidence() {
        // Bare Gathering: no optional fields filled (0/3), geo = Low (0.3)
        // confidence = 0.0 * 0.5 + 0.3 * 0.5 = 0.15
        let mut meta = test_meta();
        meta.location = None;
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: "".to_string(),
            organizer: None,
            is_recurring: false,
        });
        let q = score(&node);
        assert!(
            (q.confidence - 0.15).abs() < 0.01,
            "Bare gathering confidence: {}",
            q.confidence
        );
    }

    #[test]
    fn complete_gathering_exact_geo_scores_max() {
        // Complete Gathering: all 3 optional fields (3/3), geo = High (1.0)
        // confidence = 1.0 * 0.5 + 1.0 * 0.5 = 1.0
        let meta = test_meta(); // has exact geo location
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: "https://example.com/rsvp".to_string(),
            organizer: None,
            is_recurring: false,
        });
        let q = score(&node);
        assert!(
            (q.confidence - 1.0).abs() < 0.01,
            "Complete gathering confidence: {}",
            q.confidence
        );
    }

    #[test]
    fn notice_with_location_scores_high() {
        // Notice: only applicable optional field is location. With location filled (1/1)
        // and medium geo: confidence = 1.0 * 0.5 + 0.7 * 0.5 = 0.85
        use rootsignal_common::{NoticeNode, Severity};
        let mut meta = test_meta();
        meta.location = Some(GeoPoint {
            lat: 44.97,
            lng: -93.26,
            precision: GeoPrecision::Neighborhood,
        });
        let node = Node::Notice(NoticeNode {
            meta,
            severity: Severity::Medium,
            category: None,
            effective_date: None,
            source_authority: None,
        });
        let q = score(&node);
        assert!(
            (q.confidence - 0.85).abs() < 0.01,
            "Notice with location confidence: {}",
            q.confidence
        );
    }

    #[test]
    fn tension_without_location_scores_low() {
        // Tension: location is the only applicable field. Without it (0/1), geo = Low (0.3)
        // confidence = 0.0 * 0.5 + 0.3 * 0.5 = 0.15
        use rootsignal_common::{Severity, TensionNode};
        let mut meta = test_meta();
        meta.location = None;
        let node = Node::Tension(TensionNode {
            meta,
            severity: Severity::High,
            category: None,
            what_would_help: None,
        });
        let q = score(&node);
        assert!(
            (q.confidence - 0.15).abs() < 0.01,
            "Bare tension confidence: {}",
            q.confidence
        );
    }

    #[test]
    fn gathering_without_date_or_url_is_not_actionable() {
        let mut meta = test_meta();
        meta.source_url = "https://example.com".to_string();
        let event = Node::Gathering(GatheringNode {
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
