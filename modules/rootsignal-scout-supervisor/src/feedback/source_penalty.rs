use anyhow::Result;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_graph::GraphClient;

/// Penalty factor applied per open issue traced back to a source.
const PENALTY_PER_ISSUE: f64 = 0.15;

/// Minimum quality penalty (floor).
const MIN_PENALTY: f64 = 0.1;

/// Apply quality penalties to sources that produced signals with open validation issues.
///
/// Cross-database flow:
/// 1. Query Postgres for open issues → (target_id, count) pairs
/// 2. Query Neo4j for signal → EXTRACTED_FROM → Source using those target_ids
/// 3. Aggregate issue counts per source and apply penalties
pub async fn apply_source_penalties(client: &GraphClient, pool: &PgPool) -> Result<PenaltyStats> {
    let mut stats = PenaltyStats::default();

    // Step 1: Get open issue counts per target_id from Postgres
    let rows = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT target_id, count(*) AS issue_count
         FROM validation_issues
         WHERE status = 'open'
         GROUP BY target_id",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(stats);
    }

    // Step 2: For each target_id, find the source via Neo4j graph traversal
    // Aggregate issue counts per source canonical_key
    let mut source_issues: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();

    for (target_id, issue_count) in &rows {
        let q = neo4rs::query(
            "MATCH (sig {id: $target_id})-[:EXTRACTED_FROM]->(s:Source)
             RETURN s.canonical_key AS key",
        )
        .param("target_id", target_id.to_string());

        let mut stream = client.inner().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let key: String = row.get("key").unwrap_or_default();
            if !key.is_empty() {
                *source_issues.entry(key).or_insert(0) += issue_count;
            }
        }
    }

    // Step 3: Apply penalties
    for (key, issue_count) in &source_issues {
        let penalty = (1.0 - PENALTY_PER_ISSUE * (*issue_count as f64)).max(MIN_PENALTY);

        if let Err(e) = set_quality_penalty(client, key, penalty).await {
            warn!(source = key.as_str(), error = %e, "Failed to set quality penalty");
        } else {
            stats.sources_penalized += 1;
            info!(
                source = key.as_str(),
                penalty,
                issues = issue_count,
                "Applied quality penalty"
            );
        }
    }

    Ok(stats)
}

/// Reset quality_penalty to 1.0 for sources whose issues have all been resolved.
///
/// Cross-database flow:
/// 1. Query Neo4j for penalized sources + their signal IDs
/// 2. Batch-check Postgres for open issues against those signal IDs
/// 3. Reset any source whose signals have zero open issues
pub async fn reset_resolved_penalties(client: &GraphClient, pool: &PgPool) -> Result<u64> {
    // Step 1: Find penalized sources and their signal IDs from Neo4j
    let q = neo4rs::query(
        "MATCH (s:Source)
         WHERE s.quality_penalty < 1.0
         OPTIONAL MATCH (sig)-[:EXTRACTED_FROM]->(s)
         RETURN s.canonical_key AS key, collect(sig.id) AS signal_ids",
    );

    let mut stream = client.inner().execute(q).await?;
    let mut reset_count: u64 = 0;

    while let Some(row) = stream.next().await? {
        let key: String = row.get("key").unwrap_or_default();
        let signal_ids: Vec<String> = row.get("signal_ids").unwrap_or_default();

        if key.is_empty() {
            continue;
        }

        // Step 2: Check if any open issues exist for these signal IDs in Postgres
        let has_open_issues = if signal_ids.is_empty() {
            false
        } else {
            // Parse signal ID strings to UUIDs, skip any that don't parse
            let uuids: Vec<Uuid> = signal_ids
                .iter()
                .filter_map(|s| s.parse::<Uuid>().ok())
                .collect();

            if uuids.is_empty() {
                false
            } else {
                sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(
                        SELECT 1 FROM validation_issues
                        WHERE target_id = ANY($1) AND status = 'open'
                    )",
                )
                .bind(&uuids)
                .fetch_one(pool)
                .await?
            }
        };

        // Step 3: Reset if no open issues remain
        if !has_open_issues {
            if let Err(e) = set_quality_penalty(client, &key, 1.0).await {
                warn!(source = key.as_str(), error = %e, "Failed to reset quality penalty");
            } else {
                reset_count += 1;
            }
        }
    }

    if reset_count > 0 {
        info!(
            count = reset_count,
            "Reset quality penalties for sources with no open issues"
        );
    }

    Ok(reset_count)
}

async fn set_quality_penalty(
    client: &GraphClient,
    canonical_key: &str,
    penalty: f64,
) -> Result<(), neo4rs::Error> {
    let q = neo4rs::query(
        "MATCH (s:Source {canonical_key: $key})
         SET s.quality_penalty = $penalty",
    )
    .param("key", canonical_key)
    .param("penalty", penalty);

    client.inner().run(q).await
}

#[derive(Debug, Default)]
pub struct PenaltyStats {
    pub sources_penalized: u64,
    pub sources_reset: u64,
}
