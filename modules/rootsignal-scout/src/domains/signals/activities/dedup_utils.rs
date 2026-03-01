//! Dedup utility functions for signal processing.
//!
//! Pure functions for title normalization, batch dedup, source ownership
//! checks, signal quality scoring, and multi-layer dedup verdicts.

use std::collections::HashSet;

use rootsignal_common::types::NodeType;
use rootsignal_common::{ActorContext, Node, ScrapingStrategy};
use uuid::Uuid;

use crate::enrichment::quality;

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

/// Scores quality, populates from/about locations, and removes Evidence nodes.
///
/// Pure pipeline step: given raw extracted nodes, returns signal nodes with
/// quality scores, source URLs, and location provenance.
pub(crate) fn score_and_filter(
    mut nodes: Vec<Node>,
    url: &str,
    actor_ctx: Option<&ActorContext>,
) -> Vec<Node> {
    // 1. Score quality and stamp source URL
    for node in &mut nodes {
        let q = quality::score(node);
        if let Some(meta) = node.meta_mut() {
            meta.confidence = q.confidence;
            meta.source_url = url.to_string();
        }
    }

    // 2. Filter to signal nodes only (skip Evidence)
    nodes
        .into_iter()
        .filter(|n| !matches!(n.node_type(), NodeType::Citation))
        .collect()
}

// ---------------------------------------------------------------------------
// DedupVerdict — pure decision function for multi-layer deduplication
// ---------------------------------------------------------------------------

/// Threshold for cross-source corroboration via vector similarity.
/// Same-source matches always refresh regardless of similarity (as long as
/// they passed the 0.85 entry threshold from the caller).
const CROSS_SOURCE_SIM_THRESHOLD: f64 = 0.92;

/// The outcome of the multi-layer deduplication check for a single signal node.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DedupVerdict {
    /// No existing match — create a new signal node.
    Create,
    /// Cross-source match — corroborate the existing signal.
    Corroborate {
        existing_id: Uuid,
        existing_type: NodeType,
        similarity: f64,
    },
    /// Same-source match — refresh (re-confirm) the existing signal.
    Refresh {
        existing_id: Uuid,
        existing_type: NodeType,
        similarity: f64,
    },
}

impl DedupVerdict {
    /// Returns the existing signal ID if the verdict is not Create.
    #[cfg(test)]
    fn existing_id(&self) -> Option<Uuid> {
        match self {
            DedupVerdict::Create => None,
            DedupVerdict::Corroborate { existing_id, .. } => Some(*existing_id),
            DedupVerdict::Refresh { existing_id, .. } => Some(*existing_id),
        }
    }
}

/// Pure decision function for the multi-layer dedup pipeline.
///
/// Layers are checked in priority order:
/// 1. Global exact title+type match (similarity = 1.0)
/// 2. In-memory embed cache match (≥0.85 entry, ≥0.92 cross-source)
/// 3. Graph vector index match (≥0.85 entry, ≥0.92 cross-source)
/// 4. No match → Create
///
/// Within each layer, same-source → Refresh, cross-source above threshold → Corroborate.
/// All URLs should be pre-sanitized before calling.
pub(crate) fn dedup_verdict(
    current_url: &str,
    node_type: NodeType,
    global_match: Option<(Uuid, &str)>,
    cache_match: Option<(Uuid, NodeType, &str, f64)>,
    graph_match: Option<(Uuid, NodeType, &str, f64)>,
) -> DedupVerdict {
    // Layer 2.5: Global exact title+type match — always acts (no threshold)
    if let Some((existing_id, existing_url)) = global_match {
        return if existing_url != current_url {
            DedupVerdict::Corroborate {
                existing_id,
                existing_type: node_type,
                similarity: 1.0,
            }
        } else {
            DedupVerdict::Refresh {
                existing_id,
                existing_type: node_type,
                similarity: 1.0,
            }
        };
    }

    // Layer 3a: In-memory embed cache
    if let Some((cached_id, cached_type, cached_url, sim)) = cache_match {
        if cached_url == current_url {
            return DedupVerdict::Refresh {
                existing_id: cached_id,
                existing_type: cached_type,
                similarity: sim,
            };
        } else if sim >= CROSS_SOURCE_SIM_THRESHOLD {
            return DedupVerdict::Corroborate {
                existing_id: cached_id,
                existing_type: cached_type,
                similarity: sim,
            };
        }
    }

    // Layer 3b: Graph vector index
    if let Some((dup_id, dup_type, dup_url, sim)) = graph_match {
        if dup_url == current_url {
            return DedupVerdict::Refresh {
                existing_id: dup_id,
                existing_type: dup_type,
                similarity: sim,
            };
        } else if sim >= CROSS_SOURCE_SIM_THRESHOLD {
            return DedupVerdict::Corroborate {
                existing_id: dup_id,
                existing_type: dup_type,
                similarity: sim,
            };
        }
    }

    // Layer 4: No match
    DedupVerdict::Create
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rootsignal_common::safety::SensitivityLevel;
    use rootsignal_common::types::{GeoPoint, NeedNode, NodeMeta, Severity, TensionNode, Urgency};
    use rootsignal_common::{CitationNode, GeoPrecision, ReviewStatus, ScrapingStrategy, SocialPlatform};

    fn tension(title: &str) -> Node {
        Node::Tension(TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.8,
                corroboration_count: 0,
                about_location: None,
                about_location_name: None,
                from_location: None,
                source_url: "https://example.com".to_string(),
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
                mentioned_actors: Vec::new(),
            },
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        })
    }

    fn need(title: &str) -> Node {
        Node::Need(NeedNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.8,
                corroboration_count: 0,
                about_location: None,
                about_location_name: None,
                from_location: None,
                source_url: "https://example.com".to_string(),
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
                mentioned_actors: Vec::new(),
            },
            urgency: Urgency::Medium,
            what_needed: None,
            action_url: None,
            goal: None,
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

    // --- dedup_verdict tests ---

    const URL_A: &str = "https://example.com/page-a";
    const URL_B: &str = "https://other.com/page-b";

    fn id1() -> Uuid { Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap() }
    fn id2() -> Uuid { Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap() }
    fn id3() -> Uuid { Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap() }

    #[test]
    fn global_match_cross_source_corroborates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, Some((id1(), URL_B)), None, None);
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id1(), existing_type: NodeType::Tension, similarity: 1.0 });
    }

    #[test]
    fn global_match_same_source_refreshes() {
        let v = dedup_verdict(URL_A, NodeType::Tension, Some((id1(), URL_A)), None, None);
        assert_eq!(v, DedupVerdict::Refresh { existing_id: id1(), existing_type: NodeType::Tension, similarity: 1.0 });
    }

    #[test]
    fn global_match_uses_new_node_type() {
        let v = dedup_verdict(URL_A, NodeType::Aid, Some((id1(), URL_B)), None, None);
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id1(), existing_type: NodeType::Aid, similarity: 1.0 });
    }

    #[test]
    fn global_match_takes_priority_over_cache() {
        let v = dedup_verdict(URL_A, NodeType::Tension, Some((id1(), URL_B)), Some((id2(), NodeType::Tension, URL_A, 0.99)), None);
        assert_eq!(v.existing_id(), Some(id1()), "global match should win over cache");
    }

    #[test]
    fn global_match_takes_priority_over_graph() {
        let v = dedup_verdict(URL_A, NodeType::Tension, Some((id1(), URL_A)), None, Some((id3(), NodeType::Tension, URL_B, 0.95)));
        assert_eq!(v.existing_id(), Some(id1()), "global match should win over graph");
    }

    #[test]
    fn cache_same_source_refreshes() {
        let v = dedup_verdict(URL_A, NodeType::Need, None, Some((id2(), NodeType::Need, URL_A, 0.88)), None);
        assert_eq!(v, DedupVerdict::Refresh { existing_id: id2(), existing_type: NodeType::Need, similarity: 0.88 });
    }

    #[test]
    fn cache_cross_source_above_threshold_corroborates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_B, 0.95)), None);
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id2(), existing_type: NodeType::Tension, similarity: 0.95 });
    }

    #[test]
    fn cache_cross_source_at_threshold_corroborates() {
        let v = dedup_verdict(URL_A, NodeType::Aid, None, Some((id2(), NodeType::Aid, URL_B, 0.92)), None);
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id2(), existing_type: NodeType::Aid, similarity: 0.92 });
    }

    #[test]
    fn cache_cross_source_below_threshold_falls_through() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_B, 0.91)), None);
        assert_eq!(v, DedupVerdict::Create, "0.91 cross-source should fall through to Create");
    }

    #[test]
    fn cache_cross_source_at_entry_threshold_falls_through() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_B, 0.85)), None);
        assert_eq!(v, DedupVerdict::Create, "0.85 cross-source should fall through");
    }

    #[test]
    fn cache_takes_priority_over_graph() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_A, 0.90)), Some((id3(), NodeType::Tension, URL_B, 0.95)));
        assert_eq!(v.existing_id(), Some(id2()), "cache should win over graph");
    }

    #[test]
    fn graph_same_source_refreshes() {
        let v = dedup_verdict(URL_A, NodeType::Notice, None, None, Some((id3(), NodeType::Notice, URL_A, 0.87)));
        assert_eq!(v, DedupVerdict::Refresh { existing_id: id3(), existing_type: NodeType::Notice, similarity: 0.87 });
    }

    #[test]
    fn graph_cross_source_above_threshold_corroborates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, None, Some((id3(), NodeType::Tension, URL_B, 0.95)));
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id3(), existing_type: NodeType::Tension, similarity: 0.95 });
    }

    #[test]
    fn graph_cross_source_at_threshold_corroborates() {
        let v = dedup_verdict(URL_A, NodeType::Gathering, None, None, Some((id3(), NodeType::Gathering, URL_B, 0.92)));
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id3(), existing_type: NodeType::Gathering, similarity: 0.92 });
    }

    #[test]
    fn graph_cross_source_below_threshold_creates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, None, Some((id3(), NodeType::Tension, URL_B, 0.91)));
        assert_eq!(v, DedupVerdict::Create);
    }

    #[test]
    fn no_matches_creates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, None, None);
        assert_eq!(v, DedupVerdict::Create);
    }

    #[test]
    fn both_below_threshold_creates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_B, 0.87)), Some((id3(), NodeType::Tension, URL_B, 0.89)));
        assert_eq!(v, DedupVerdict::Create);
    }

    #[test]
    fn cache_below_threshold_falls_to_graph_refresh() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_B, 0.87)), Some((id3(), NodeType::Tension, URL_A, 0.90)));
        assert_eq!(v, DedupVerdict::Refresh { existing_id: id3(), existing_type: NodeType::Tension, similarity: 0.90 });
    }

    #[test]
    fn cache_below_threshold_falls_to_graph_corroborate() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, Some((id2(), NodeType::Tension, URL_B, 0.88)), Some((id3(), NodeType::Tension, URL_B, 0.93)));
        assert_eq!(v, DedupVerdict::Corroborate { existing_id: id3(), existing_type: NodeType::Tension, similarity: 0.93 });
    }

    // --- score_and_filter tests ---

    fn tension_at(title: &str, lat: f64, lng: f64) -> Node {
        Node::Tension(TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0,
                corroboration_count: 0,
                about_location: Some(GeoPoint { lat, lng, precision: GeoPrecision::Approximate }),
                about_location_name: None,
                from_location: None,
                source_url: String::new(),
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
                mentioned_actors: Vec::new(),
            },
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        })
    }

    fn tension_with_name(title: &str, location_name: &str) -> Node {
        Node::Tension(TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0,
                corroboration_count: 0,
                about_location: None,
                about_location_name: Some(location_name.to_string()),
                from_location: None,
                source_url: String::new(),
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
                mentioned_actors: Vec::new(),
            },
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        })
    }

    #[test]
    fn score_filter_signal_stored_regardless_of_location() {
        let nodes = vec![tension_at("Pothole on Lake St", 44.9485, -93.2983)];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn score_filter_out_of_region_signal_still_stored() {
        let nodes = vec![tension_at("NYC subway delay", 40.7128, -74.0060)];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn score_filter_stamps_source_url() {
        let nodes = vec![tension_at("Test signal", 44.95, -93.27)];
        let result = score_and_filter(nodes, "https://mpls-news.com/article", None);
        assert_eq!(result[0].meta().unwrap().source_url, "https://mpls-news.com/article");
    }

    #[test]
    fn score_filter_sets_confidence() {
        let nodes = vec![tension_at("Test signal", 44.95, -93.27)];
        let result = score_and_filter(nodes, URL_A, None);
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
        let result = score_and_filter(nodes, URL_A, None);
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
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        assert_eq!(result.len(), 1);
        let meta = result[0].meta().unwrap();
        assert!(meta.about_location.is_none(), "about_location should NOT be backfilled from actor");
        assert!(meta.from_location.is_none(), "from_location should NOT be set at write time");
    }

    #[test]
    fn score_filter_no_location_no_actor_still_stored() {
        let nodes = vec![tension("Floating Signal")];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
        let meta = result[0].meta().unwrap();
        assert!(meta.about_location.is_none());
        assert!(meta.from_location.is_none());
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
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        let meta = result[0].meta().unwrap();
        let about = meta.about_location.as_ref().unwrap();
        assert!((about.lat - 44.95).abs() < 0.001, "existing about_location should be preserved");
        assert!(meta.from_location.is_none(), "from_location should NOT be set at write time");
    }

    #[test]
    fn score_filter_all_signals_stored_regardless_of_region() {
        let nodes = vec![
            tension_at("Minneapolis", 44.95, -93.27),
            tension_at("Los Angeles", 34.05, -118.24),
            tension_at("Also Minneapolis", 44.98, -93.25),
        ];
        let result = score_and_filter(nodes, URL_A, None);
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
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        for node in &result {
            let meta = node.meta().unwrap();
            assert!(meta.from_location.is_none(), "{} should NOT have from_location at write time", meta.title);
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
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        let meta = result[0].meta().unwrap();
        assert!(meta.about_location.is_none());
        assert!(meta.from_location.is_none());
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
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1, "evidence nodes should be filtered out");
        assert_eq!(result[0].title(), "Real Signal");
    }

    #[test]
    fn source_url_stamped_on_all_signals() {
        let nodes = vec![tension("Signal A"), tension("Signal B")];
        let result = score_and_filter(nodes, "https://test-source.org", None);
        for node in &result {
            let meta = node.meta().unwrap();
            assert_eq!(meta.source_url, "https://test-source.org");
        }
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
