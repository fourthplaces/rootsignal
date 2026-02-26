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

use anyhow::Result;
use neo4rs::query;
use tracing::info;

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
    threshold: f64,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> Result<EnrichStats> {
    let mut stats = EnrichStats::default();

    // 1. Diversity: count Evidence edges per entity → SET diversity properties
    stats.diversity_updated = compute_diversity(client).await?;

    // 2. Actor stats: count ACTED_IN edges per actor → SET signal_count
    stats.actor_stats_updated = compute_actor_stats(client).await?;

    // 3. Cause heat: read embeddings + diversity, compute heats → SET cause_heat
    //    (depends on diversity being written first)
    stats.cause_heat_updated =
        crate::cause_heat::compute_cause_heat(client, threshold, min_lat, max_lat, min_lng, max_lng)
            .await
            .map(|_| 0u32) // cause_heat doesn't return a count yet; TODO: return count
            .unwrap_or(0);

    info!(?stats, "Enrichment complete");
    Ok(stats)
}

/// Compute source diversity for all signal types.
///
/// For each entity with SOURCED_FROM→Evidence edges:
/// - source_diversity = count of distinct source URLs
/// - channel_diversity = count of distinct channel types
/// - external_ratio = fraction of non-primary evidence sources
///
/// Runs a single Cypher query per entity label.
async fn compute_diversity(client: &GraphClient) -> Result<u32> {
    let g = &client.graph;
    let mut updated = 0u32;

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})-[:SOURCED_FROM]->(ev:Evidence)
             WITH n,
                  count(DISTINCT ev.source_url) AS src_div,
                  count(DISTINCT ev.channel_type) AS ch_div,
                  count(ev) AS total,
                  count(CASE WHEN ev.relevance <> 'primary' THEN 1 END) AS non_primary
             SET n.source_diversity = src_div,
                 n.channel_diversity = ch_div,
                 n.external_ratio = CASE WHEN total = 0 THEN 0.0
                                         ELSE toFloat(non_primary) / toFloat(total) END
             RETURN count(n) AS cnt"
        ));

        let mut stream = g.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            updated += cnt as u32;
        }
    }

    info!(updated, "Diversity enrichment complete");
    Ok(updated)
}

/// Compute actor stats from ACTED_IN edges.
///
/// signal_count = number of ACTED_IN edges (entities the actor participated in)
/// last_active = max timestamp from ACTED_IN edges (if available) or preserved
async fn compute_actor_stats(client: &GraphClient) -> Result<u32> {
    let g = &client.graph;

    let q = query(
        "MATCH (a:Actor)-[r:ACTED_IN]->()
         WITH a, count(r) AS cnt
         SET a.signal_count = cnt
         RETURN count(a) AS updated"
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
    // Integration tests requiring Neo4j live in modules/rootsignal-graph/tests/enrich_test.rs
    // Unit tests here would require mocking the GraphClient, which isn't worth the complexity
    // given that these are thin Cypher wrappers. The contract is tested by the integration tests.
}
