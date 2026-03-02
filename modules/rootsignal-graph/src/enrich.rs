//! Pure diversity metric computation.
//!
//! The `compute_diversity_metrics` function is used by the diversity activity
//! to compute metrics from graph evidence. It's a pure function — no I/O.

use std::collections::HashSet;

use rootsignal_common::{resolve_entity, EntityMappingOwned};

/// Diversity metrics for a single signal node.
#[derive(Debug, Clone, PartialEq)]
pub struct DiversityMetrics {
    pub source_diversity: i64,
    pub channel_diversity: i64,
    pub external_ratio: f64,
}

/// Pure computation: given a signal's own URL and its evidence (url, channel) pairs,
/// compute diversity metrics using entity resolution.
///
/// - source_diversity = count of distinct source *entities* (domain-based grouping)
/// - channel_diversity = count of distinct channel types with at least one external entity
/// - external_ratio = fraction of evidence from entities other than the signal's own source
pub fn compute_diversity_metrics(
    self_url: &str,
    evidence: &[(String, String)],
    entity_mappings: &[EntityMappingOwned],
) -> DiversityMetrics {
    let self_entity = resolve_entity(self_url, entity_mappings);

    let mut entities = HashSet::new();
    let mut external_count = 0u32;
    let mut channels_with_external = HashSet::new();

    for (url, channel) in evidence {
        let entity = resolve_entity(url, entity_mappings);
        entities.insert(entity.clone());
        if entity != self_entity {
            external_count += 1;
            channels_with_external.insert(channel.clone());
        }
    }

    let total = evidence.len() as u32;

    DiversityMetrics {
        source_diversity: entities.len() as i64,
        channel_diversity: channels_with_external.len() as i64,
        external_ratio: if total > 0 {
            external_count as f64 / total as f64
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(url: &str, channel: &str) -> (String, String) {
        (url.to_string(), channel.to_string())
    }

    #[test]
    fn no_evidence_returns_zero_diversity() {
        let m = compute_diversity_metrics("https://example.com/article", &[], &[]);
        assert_eq!(m.source_diversity, 0);
        assert_eq!(m.channel_diversity, 0);
        assert_eq!(m.external_ratio, 0.0);
    }

    #[test]
    fn single_same_domain_evidence_is_not_external() {
        let evidence = vec![ev("https://example.com/other", "press")];
        let m = compute_diversity_metrics("https://example.com/article", &evidence, &[]);
        assert_eq!(m.source_diversity, 1); // same domain = same entity
        assert_eq!(m.external_ratio, 0.0);
    }

    #[test]
    fn different_domains_count_as_separate_entities() {
        let evidence = vec![
            ev("https://example.com/a", "press"),
            ev("https://other.org/b", "press"),
            ev("https://third.net/c", "press"),
        ];
        let m = compute_diversity_metrics("https://example.com/article", &evidence, &[]);
        assert_eq!(m.source_diversity, 3);
        assert_eq!(m.external_ratio, 2.0 / 3.0);
    }

    #[test]
    fn channel_diversity_only_counts_channels_with_external_entities() {
        let evidence = vec![
            ev("https://example.com/a", "press"),    // same entity, press
            ev("https://other.org/b", "press"),      // external, press
            ev("https://example.com/c", "social"),   // same entity, social — not counted
            ev("https://third.net/d", "government"), // external, government
        ];
        let m = compute_diversity_metrics("https://example.com/article", &evidence, &[]);
        // channels with external: press (other.org), government (third.net) → 2
        assert_eq!(m.channel_diversity, 2);
    }

    #[test]
    fn entity_mapping_groups_subdomains_into_one_entity() {
        let mappings = vec![EntityMappingOwned {
            canonical_key: "big-media".to_string(),
            domains: vec!["news.big.com".to_string(), "opinion.big.com".to_string()],
            instagram: vec![],
            facebook: vec![],
            reddit: vec![],
        }];
        let evidence = vec![
            ev("https://news.big.com/story", "press"),
            ev("https://opinion.big.com/take", "press"),
        ];
        let m = compute_diversity_metrics("https://news.big.com/original", &evidence, &mappings);
        // Both resolve to "big-media" entity, same as self → all internal
        assert_eq!(m.source_diversity, 1);
        assert_eq!(m.external_ratio, 0.0);
    }

    #[test]
    fn mixed_internal_and_external_computes_correct_ratio() {
        let evidence = vec![
            ev("https://example.com/a", "press"),
            ev("https://example.com/b", "press"),
            ev("https://external.org/c", "social"),
        ];
        let m = compute_diversity_metrics("https://example.com/article", &evidence, &[]);
        assert_eq!(m.source_diversity, 2); // example.com + external.org
        assert_eq!(m.external_ratio, 1.0 / 3.0);
        assert_eq!(m.channel_diversity, 1); // only social has external
    }
}
