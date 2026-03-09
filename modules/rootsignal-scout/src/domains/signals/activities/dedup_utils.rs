//! Dedup utility functions for signal processing.
//!
//! Pure functions for title normalization, batch dedup, source ownership
//! checks, and signal quality scoring.

use std::collections::HashSet;

use rootsignal_common::types::NodeType;
use rootsignal_common::{ActorContext, Node, ScrapingStrategy};

use crate::domains::enrichment::activities::quality;

/// Normalize a title for dedup comparison: lowercase and trim.
pub(crate) fn normalize_title(title: &str) -> String {
    title.trim().to_lowercase()
}

/// Within-batch dedup by (normalized_title, node_type).
/// Keeps the first occurrence of each (title, type) pair, drops duplicates.
pub(crate) fn batch_title_dedup(nodes: Vec<Node>) -> Vec<Node> {
    let mut seen = HashSet::new();
    nodes
        .into_iter()
        .filter(|n| seen.insert((normalize_title(n.title()), n.node_type())))
        .collect()
}

/// Returns true if this scraping strategy represents an "owned" source — one
/// where the author of the content is the account holder, not an aggregator.
/// Social accounts and dedicated web pages are owned; RSS feeds and web
/// queries aggregate content from many authors.
pub(crate) fn is_owned_source(strategy: &ScrapingStrategy) -> bool {
    matches!(strategy, ScrapingStrategy::Social(_))
}

/// Scores quality and removes Citation nodes.
///
/// Pure pipeline step: given raw extracted nodes, returns signal nodes with
/// quality scores assigned. Does not modify source URLs.
pub(crate) fn score_and_filter(
    mut nodes: Vec<Node>,
    actor_ctx: Option<&ActorContext>,
) -> Vec<Node> {
    for node in &mut nodes {
        let q = quality::score(node);
        if let Some(meta) = node.meta_mut() {
            meta.confidence = q.confidence;
        }
    }

    nodes
        .into_iter()
        .filter(|n| !matches!(n.node_type(), NodeType::Citation))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rootsignal_common::safety::SensitivityLevel;
    use rootsignal_common::types::{GeoPoint, HelpRequestNode, NodeMeta, Severity, ConcernNode, Urgency};
    use rootsignal_common::{CitationNode, GeoPrecision, Location, ReviewStatus, ScrapingStrategy, SocialPlatform};
    use uuid::Uuid;

    fn tension(title: &str) -> Node {
        Node::Concern(ConcernNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.8,
                corroboration_count: 0,
                locations: vec![],
                url: "https://example.com".to_string(),
                extracted_at: Utc::now(),
                published_at: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                review_status: ReviewStatus::Staged,
                was_corrected: false,
                corrections: None,
                rejection_reason: None,
                mentioned_entities: vec![],
                category: None,
            },
            severity: Severity::Medium,
            subject: None,
            opposing: None,
        })
    }

    fn need(title: &str) -> Node {
        Node::HelpRequest(HelpRequestNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.8,
                corroboration_count: 0,
                locations: vec![],
                url: "https://example.com".to_string(),
                extracted_at: Utc::now(),
                published_at: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                review_status: ReviewStatus::Staged,
                was_corrected: false,
                corrections: None,
                rejection_reason: None,
                mentioned_entities: vec![],
                category: None,
            },
            urgency: Urgency::Medium,
            what_needed: None,
            action_url: None,
            stated_goal: None,
        })
    }

    // --- normalize_title tests ---

    #[test]
    fn normalize_title_trims_whitespace() {
        assert_eq!(normalize_title("  Free Legal Clinic  "), "free legal clinic");
    }

    #[test]
    fn normalize_title_lowercases() {
        assert_eq!(normalize_title("FREE LEGAL CLINIC"), "free legal clinic");
    }

    #[test]
    fn normalize_title_mixed_case_and_whitespace() {
        assert_eq!(normalize_title("  Community Garden CLEANUP  "), "community garden cleanup");
    }

    #[test]
    fn normalize_title_empty() {
        assert_eq!(normalize_title(""), "");
    }

    #[test]
    fn normalize_title_already_normalized() {
        assert_eq!(normalize_title("food distribution"), "food distribution");
    }

    // --- batch_title_dedup tests ---

    #[test]
    fn batch_dedup_removes_same_title_same_type() {
        let nodes = vec![tension("Housing Crisis"), tension("Housing Crisis"), tension("Bus Route Cut")];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].title(), "Housing Crisis");
        assert_eq!(deduped[1].title(), "Bus Route Cut");
    }

    #[test]
    fn batch_dedup_keeps_same_title_different_type() {
        let nodes = vec![tension("Housing Crisis"), need("Housing Crisis")];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn batch_dedup_case_insensitive() {
        let nodes = vec![tension("housing crisis"), tension("HOUSING CRISIS"), tension("Housing Crisis")];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn batch_dedup_whitespace_normalized() {
        let nodes = vec![tension("  Housing Crisis  "), tension("Housing Crisis")];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn batch_dedup_empty_input() {
        let deduped = batch_title_dedup(Vec::new());
        assert!(deduped.is_empty());
    }

    #[test]
    fn batch_dedup_all_unique() {
        let nodes = vec![tension("Housing Crisis"), tension("Bus Route Cut"), need("Food Distribution")];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 3);
    }

    // --- score_and_filter tests ---

    fn tension_at(title: &str, lat: f64, lng: f64) -> Node {
        Node::Concern(ConcernNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0,
                corroboration_count: 0,
                locations: vec![Location { point: Some(GeoPoint { lat, lng, precision: GeoPrecision::Approximate }), name: None, address: None, role: None }],
                url: String::new(),
                extracted_at: Utc::now(),
                published_at: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                review_status: ReviewStatus::Staged,
                was_corrected: false,
                corrections: None,
                rejection_reason: None,
                mentioned_entities: vec![],
                category: None,
            },
            severity: Severity::Medium,
            subject: None,
            opposing: None,
        })
    }

    fn tension_with_name(title: &str, location_name: &str) -> Node {
        Node::Concern(ConcernNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0,
                corroboration_count: 0,
                locations: vec![Location { point: None, name: Some(location_name.to_string()), address: None, role: None }],
                url: String::new(),
                extracted_at: Utc::now(),
                published_at: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                review_status: ReviewStatus::Staged,
                was_corrected: false,
                corrections: None,
                rejection_reason: None,
                mentioned_entities: vec![],
                category: None,
            },
            severity: Severity::Medium,
            subject: None,
            opposing: None,
        })
    }

    #[test]
    fn score_filter_signal_stored_regardless_of_location() {
        let nodes = vec![tension_at("Pothole on Lake St", 44.9485, -93.2983)];
        let result = score_and_filter(nodes, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn score_filter_out_of_region_signal_still_stored() {
        let nodes = vec![tension_at("NYC subway delay", 40.7128, -74.0060)];
        let result = score_and_filter(nodes, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn score_filter_sets_confidence() {
        let nodes = vec![tension_at("Test signal", 44.95, -93.27)];
        let result = score_and_filter(nodes, None);
        assert!(result[0].meta().unwrap().confidence > 0.0, "confidence should be set by quality::score");
    }

    #[test]
    fn score_filter_removes_evidence_nodes() {
        let evidence = Node::Citation(CitationNode {
            id: Uuid::new_v4(),
            source_url: "https://example.com".to_string(),
            retrieved_at: Utc::now(),
            content_hash: "abc".to_string(),
            snippet: None,
            relevance: None,
            confidence: None,
            channel_type: None,
        });
        let nodes = vec![tension_at("Real signal", 44.95, -93.27), evidence];
        let result = score_and_filter(nodes, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title(), "Real signal");
    }

    #[test]
    fn score_filter_does_not_fabricate_about_location_from_actor() {
        let nodes = vec![tension_with_name("Local Event", "Minneapolis")];
        let actor = ActorContext {
            actor_name: "Local Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, Some(&actor));
        assert_eq!(result.len(), 1);
        let meta = result[0].meta().unwrap();
        assert!(meta.about_point().is_none(), "about_location should NOT be backfilled from actor");
        assert!(meta.from_point().is_none(), "from_location should NOT be set at write time");
    }

    #[test]
    fn score_filter_no_location_no_actor_still_stored() {
        let nodes = vec![tension("Floating Signal")];
        let result = score_and_filter(nodes, None);
        assert_eq!(result.len(), 1);
        let meta = result[0].meta().unwrap();
        assert!(meta.about_point().is_none());
        assert!(meta.from_point().is_none());
    }

    #[test]
    fn score_filter_actor_preserves_existing_about_location() {
        let nodes = vec![tension_at("Located Signal", 44.95, -93.28)];
        let actor = ActorContext {
            actor_name: "Far Away Org".to_string(),
            bio: None,
            location_name: None,
            location_lat: Some(40.7128),
            location_lng: Some(-74.0060),
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, Some(&actor));
        let meta = result[0].meta().unwrap();
        let about = meta.about_point().unwrap();
        assert!((about.lat - 44.95).abs() < 0.001, "existing about_location should be preserved");
        assert!(meta.from_point().is_none(), "from_location should NOT be set at write time");
    }

    #[test]
    fn score_filter_all_signals_stored_regardless_of_region() {
        let nodes = vec![
            tension_at("Minneapolis", 44.95, -93.27),
            tension_at("Los Angeles", 34.05, -118.24),
            tension_at("Also Minneapolis", 44.98, -93.25),
        ];
        let result = score_and_filter(nodes, None);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn score_filter_does_not_set_from_location() {
        let nodes = vec![tension_at("Uptown Event", 44.95, -93.30), tension("Floating Signal")];
        let actor = ActorContext {
            actor_name: "Local Org".to_string(),
            bio: None,
            location_name: None,
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, Some(&actor));
        for node in &result {
            let meta = node.meta().unwrap();
            assert!(meta.from_point().is_none(), "{} should NOT have from_location at write time", meta.title);
        }
    }

    #[test]
    fn actor_without_coords_does_not_set_locations() {
        let nodes = vec![tension("No Location Signal")];
        let actor = ActorContext {
            actor_name: "Anonymous Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: None,
            location_lng: None,
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, Some(&actor));
        let meta = result[0].meta().unwrap();
        assert!(meta.about_point().is_none());
        assert!(meta.from_point().is_none());
    }

    #[test]
    fn evidence_nodes_are_filtered_out() {
        let evidence = Node::Citation(CitationNode {
            id: Uuid::new_v4(),
            content_hash: "abc".to_string(),
            source_url: "https://example.com".to_string(),
            retrieved_at: Utc::now(),
            snippet: None,
            relevance: None,
            confidence: None,
            channel_type: None,
        });
        let nodes = vec![tension("Real Signal"), evidence];
        let result = score_and_filter(nodes, None);
        assert_eq!(result.len(), 1, "evidence nodes should be filtered out");
        assert_eq!(result[0].title(), "Real Signal");
    }

    // --- is_owned_source tests ---

    #[test]
    fn is_owned_source_social_returns_true() {
        assert!(is_owned_source(&ScrapingStrategy::Social(SocialPlatform::Instagram)));
        assert!(is_owned_source(&ScrapingStrategy::Social(SocialPlatform::Facebook)));
        assert!(is_owned_source(&ScrapingStrategy::Social(SocialPlatform::Twitter)));
    }

    #[test]
    fn is_owned_source_web_page_returns_false() {
        assert!(!is_owned_source(&ScrapingStrategy::WebPage));
    }

    #[test]
    fn is_owned_source_rss_returns_false() {
        assert!(!is_owned_source(&ScrapingStrategy::Rss));
    }

    #[test]
    fn is_owned_source_web_query_returns_false() {
        assert!(!is_owned_source(&ScrapingStrategy::WebQuery));
    }
}
