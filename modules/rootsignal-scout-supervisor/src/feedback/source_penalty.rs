use anyhow::Result;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use rootsignal_common::events::{SystemEvent, SystemSourceChange};
use rootsignal_graph::GraphClient;

/// Penalty factor applied per open issue traced back to a source.
const PENALTY_PER_ISSUE: f64 = 0.15;

/// Minimum quality penalty (floor).
const MIN_PENALTY: f64 = 0.1;

/// Compute quality penalty events for sources that produced signals with open validation issues.
///
/// Cross-database flow:
/// 1. Query Postgres for open issues → (target_id, count) pairs
/// 2. Query Neo4j for signal → EXTRACTED_FROM → Source using those target_ids
/// 3. Aggregate issue counts per source and compute penalties
/// 4. Return events — the caller persists them and the GraphProjector applies them
pub async fn apply_source_penalties(
    client: &GraphClient,
    pool: &PgPool,
) -> Result<(PenaltyStats, Vec<SystemEvent>)> {
    let mut stats = PenaltyStats::default();
    let mut events = Vec::new();

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
        return Ok((stats, events));
    }

    // Step 2: For each target_id, find the source via Neo4j graph traversal
    // Aggregate issue counts per source canonical_key
    let mut source_issues: std::collections::HashMap<String, (Uuid, i64, f64)> =
        std::collections::HashMap::new();

    for (target_id, issue_count) in &rows {
        let q = neo4rs::query(
            "MATCH (sig {id: $target_id})-[:EXTRACTED_FROM]->(s:Source)
             RETURN s.id AS id, s.canonical_key AS key,
                    coalesce(s.quality_penalty, 1.0) AS current_penalty",
        )
        .param("target_id", target_id.to_string());

        let mut stream = client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let key: String = row.get("key").unwrap_or_default();
            let current_penalty: f64 = row.get("current_penalty").unwrap_or(1.0);
            if !key.is_empty() {
                let source_id = id_str.parse::<Uuid>().unwrap_or_else(|_| Uuid::nil());
                let entry = source_issues.entry(key).or_insert((source_id, 0, current_penalty));
                entry.1 += issue_count;
            }
        }
    }

    // Step 3: Compute penalties and return events
    for (key, (source_id, issue_count, current_penalty)) in &source_issues {
        let new_penalty = (1.0 - PENALTY_PER_ISSUE * (*issue_count as f64)).max(MIN_PENALTY);

        stats.sources_penalized += 1;
        info!(
            source = key.as_str(),
            penalty = new_penalty,
            issues = issue_count,
            "Computed quality penalty"
        );

        events.push(SystemEvent::SourceSystemChanged {
            source_id: *source_id,
            canonical_key: key.clone(),
            change: SystemSourceChange::QualityPenalty {
                old: *current_penalty,
                new: new_penalty,
            },
        });
    }

    Ok((stats, events))
}

/// Compute reset events for sources whose issues have all been resolved.
///
/// Cross-database flow:
/// 1. Query Neo4j for penalized sources + their signal IDs
/// 2. Batch-check Postgres for open issues against those signal IDs
/// 3. Return reset events for sources with zero open issues
pub async fn reset_resolved_penalties(
    client: &GraphClient,
    pool: &PgPool,
) -> Result<(u64, Vec<SystemEvent>)> {
    let mut events = Vec::new();

    // Step 1: Find penalized sources and their signal IDs from Neo4j
    let q = neo4rs::query(
        "MATCH (s:Source)
         WHERE s.quality_penalty < 1.0
         OPTIONAL MATCH (sig)-[:EXTRACTED_FROM]->(s)
         RETURN s.id AS source_id, s.canonical_key AS key,
                s.quality_penalty AS current_penalty,
                collect(sig.id) AS signal_ids",
    );

    let mut stream = client.execute(q).await?;

    while let Some(row) = stream.next().await? {
        let source_id_str: String = row.get("source_id").unwrap_or_default();
        let key: String = row.get("key").unwrap_or_default();
        let current_penalty: f64 = row.get("current_penalty").unwrap_or(1.0);
        let signal_ids: Vec<String> = row.get("signal_ids").unwrap_or_default();

        if key.is_empty() {
            continue;
        }

        let source_id = source_id_str
            .parse::<Uuid>()
            .unwrap_or_else(|_| Uuid::nil());

        // Step 2: Check if any open issues exist for these signal IDs in Postgres
        let has_open_issues = if signal_ids.is_empty() {
            false
        } else {
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

        // Step 3: Emit reset event if no open issues remain
        if !has_open_issues {
            events.push(SystemEvent::SourceSystemChanged {
                source_id,
                canonical_key: key,
                change: SystemSourceChange::QualityPenalty {
                    old: current_penalty,
                    new: 1.0,
                },
            });
        }
    }

    let reset_count = events.len() as u64;
    if reset_count > 0 {
        info!(
            count = reset_count,
            "Computed penalty resets for sources with no open issues"
        );
    }

    Ok((reset_count, events))
}

#[derive(Debug, Default)]
pub struct PenaltyStats {
    pub sources_penalized: u64,
    pub sources_reset: u64,
}
