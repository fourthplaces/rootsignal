//! Enrichment passes — compute derived properties and write them directly to Neo4j.
//!
//! Enrichment reads graph state built by the reducer and writes derived metrics
//! (diversity, actor stats, cause_heat) as SET operations on existing nodes.
//! These properties are deterministically recomputable from graph state — on replay,
//! enrichment runs fresh after the reducer finishes.
//!
//! Ownership boundary:
//! - Reducer: factual properties (title, summary, confidence, corroboration_count, etc.)
//! - Enrichment: derived properties (source_diversity, channel_diversity, external_ratio,
//!   cause_heat, signal_count)

use std::collections::HashSet;

use anyhow::Result;
use neo4rs::query;
use tracing::info;

use rootsignal_common::{resolve_entity, EntityMappingOwned};

use crate::GraphClient;

/// Stats from a full enrichment run.
#[derive(Debug, Default)]
pub struct EnrichStats {
    pub diversity_updated: u32,
    pub actor_stats_updated: u32,
    pub cause_heat_updated: u32,
}

/// Run all enrichment passes. Order matters: diversity must run before cause_heat
/// because cause_heat reads source_diversity from the graph.
pub async fn enrich(
    client: &GraphClient,
    entity_mappings: &[EntityMappingOwned],
    threshold: f64,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> Result<EnrichStats> {
    let mut stats = EnrichStats::default();

    // 1. Diversity: resolve entities from evidence URLs, compute metrics, write back
    stats.diversity_updated = compute_diversity(client, entity_mappings).await?;

    // 2. Actor stats: count ACTED_IN edges per actor → SET signal_count
    stats.actor_stats_updated = compute_actor_stats(client).await?;

    // 3. Cause heat: read embeddings + diversity, compute heats → SET cause_heat
    //    (depends on diversity being written first)
    stats.cause_heat_updated = crate::cause_heat::compute_cause_heat(
        client, threshold, min_lat, max_lat, min_lng, max_lng,
    )
    .await
    .map(|_| 0u32) // cause_heat doesn't return a count yet
    .unwrap_or(0);

    info!(?stats, "Enrichment complete");
    Ok(stats)
}

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

/// Compute source diversity, channel diversity, and external ratio for all signal types.
///
/// Uses the same entity resolution logic as the scout pipeline (resolve_entity).
/// Batch-loads evidence per label, computes in Rust, writes back via UNWIND.
async fn compute_diversity(
    client: &GraphClient,
    entity_mappings: &[EntityMappingOwned],
) -> Result<u32> {
    let g = &client.graph;
    let mut updated = 0u32;

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
        // Batch-load: each signal with its source_url and all evidence (url + channel_type)
        let q = query(&format!(
            "MATCH (n:{label})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             RETURN n.id AS id, n.source_url AS self_url,
                    collect({{url: ev.source_url, channel: coalesce(ev.channel_type, 'press')}}) AS evidence"
        ));

        // Collect all rows first to avoid holding the stream across awaits
        let mut rows: Vec<(String, String, Vec<(String, String)>)> = Vec::new();
        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let self_url: String = row.get("self_url").unwrap_or_default();
            let evidence: Vec<neo4rs::BoltMap> = row.get("evidence").unwrap_or_default();

            let ev_pairs: Vec<(String, String)> = evidence
                .iter()
                .filter_map(|ev| {
                    let url: String = ev.get("url").unwrap_or_default();
                    if url.is_empty() {
                        return None;
                    }
                    let channel: String = ev
                        .get::<String>("channel")
                        .unwrap_or_else(|_| "press".to_string());
                    Some((url, channel))
                })
                .collect();

            rows.push((id, self_url, ev_pairs));
        }

        // Compute metrics in Rust, collect for batch write
        let mut batch: Vec<(String, DiversityMetrics)> = Vec::with_capacity(rows.len());
        for (id, self_url, evidence) in &rows {
            let metrics = compute_diversity_metrics(self_url, evidence, entity_mappings);
            batch.push((id.clone(), metrics));
        }

        if batch.is_empty() {
            continue;
        }

        // Batch write via UNWIND — one Cypher call per label
        let params: Vec<neo4rs::BoltType> = batch
            .iter()
            .map(|(id, m)| {
                neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                    (
                        neo4rs::BoltString::from("id"),
                        neo4rs::BoltType::String(neo4rs::BoltString::from(id.as_str())),
                    ),
                    (
                        neo4rs::BoltString::from("src_div"),
                        neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(m.source_diversity)),
                    ),
                    (
                        neo4rs::BoltString::from("ch_div"),
                        neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(m.channel_diversity)),
                    ),
                    (
                        neo4rs::BoltString::from("ext_ratio"),
                        neo4rs::BoltType::Float(neo4rs::BoltFloat::new(m.external_ratio)),
                    ),
                ]))
            })
            .collect();

        let write_q = query(&format!(
            "UNWIND $rows AS row
             MATCH (n:{label} {{id: row.id}})
             SET n.source_diversity = row.src_div,
                 n.channel_diversity = row.ch_div,
                 n.external_ratio = row.ext_ratio"
        ))
        .param("rows", params);

        g.run(write_q).await?;
        updated += batch.len() as u32;
    }

    info!(updated, "Diversity enrichment complete");
    Ok(updated)
}

/// Compute actor stats from ACTED_IN edges.
///
/// signal_count = number of ACTED_IN edges (entities the actor participated in).
async fn compute_actor_stats(client: &GraphClient) -> Result<u32> {
    let g = &client.graph;

    let q = query(
        "MATCH (a:Actor)-[r:ACTED_IN]->()
         WITH a, count(r) AS cnt
         SET a.signal_count = cnt
         RETURN count(a) AS updated",
    );

    let mut stream = g.execute(q).await?;
    let updated = if let Some(row) = stream.next().await? {
        let cnt: i64 = row.get("updated").unwrap_or(0);
        cnt as u32
    } else {
        0
    };

    info!(updated, "Actor stats enrichment complete");
    Ok(updated)
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
