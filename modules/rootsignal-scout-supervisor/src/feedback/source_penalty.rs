use anyhow::Result;
use tracing::{info, warn};

use rootsignal_graph::GraphClient;

/// Penalty factor applied per open issue traced back to a source.
const PENALTY_PER_ISSUE: f64 = 0.15;

/// Minimum quality penalty (floor).
const MIN_PENALTY: f64 = 0.1;

/// Apply quality penalties to sources that produced signals with open validation issues.
///
/// Traces from open ValidationIssue nodes → target Signal → EXTRACTED_FROM → Source,
/// then sets quality_penalty = max(MIN_PENALTY, 1.0 - PENALTY_PER_ISSUE * issue_count).
pub async fn apply_source_penalties(client: &GraphClient) -> Result<PenaltyStats> {
    let mut stats = PenaltyStats::default();

    // Find sources with open issues against their signals, grouped by source
    let q = neo4rs::query(
        "MATCH (v:ValidationIssue {status: 'open'})
         MATCH (sig {id: v.target_id})-[:EXTRACTED_FROM]->(s:Source)
         WITH s.canonical_key AS key, count(v) AS issue_count
         RETURN key, issue_count",
    );

    let mut stream = client.inner().execute(q).await?;
    while let Some(row) = stream.next().await? {
        let key: String = row.get("key").unwrap_or_default();
        let issue_count: i64 = row.get("issue_count").unwrap_or(0);

        if key.is_empty() {
            continue;
        }

        let penalty = (1.0 - PENALTY_PER_ISSUE * (issue_count as f64)).max(MIN_PENALTY);

        if let Err(e) = set_quality_penalty(client, &key, penalty).await {
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
pub async fn reset_resolved_penalties(client: &GraphClient) -> Result<u64> {
    // Find penalized sources that no longer have open issues tracing back to them
    let q = neo4rs::query(
        "MATCH (s:Source)
         WHERE s.quality_penalty < 1.0
         AND NOT EXISTS {
             MATCH (v:ValidationIssue {status: 'open'})
             MATCH (sig {id: v.target_id})-[:EXTRACTED_FROM]->(s)
         }
         SET s.quality_penalty = 1.0
         RETURN count(s) AS reset_count",
    );

    let mut stream = client.inner().execute(q).await?;
    let count = if let Some(row) = stream.next().await? {
        row.get::<i64>("reset_count").unwrap_or(0) as u64
    } else {
        0
    };

    if count > 0 {
        info!(
            count,
            "Reset quality penalties for sources with no open issues"
        );
    }

    Ok(count)
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
