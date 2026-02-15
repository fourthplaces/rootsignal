use anyhow::Result;
use pgvector::Vector;
use rootsignal_core::ServerDeps;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::findings::activities::investigate::{run_why_investigation, InvestigationTrigger};
use crate::search::Embedding;

#[derive(Debug, sqlx::FromRow)]
struct CityCluster {
    city: String,
    signal_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct SignalId {
    id: Uuid,
}

/// Detect clusters of signals that exceed the baseline and warrant investigation.
pub async fn detect_signal_clusters(deps: &Arc<ServerDeps>) -> Result<Vec<Uuid>> {
    let pool = deps.pool();
    let mut triggered_signals = Vec::new();

    // 1. Query signals created in last 7 days, grouped by city
    // Compare against 30-day rolling baseline
    let clusters = sqlx::query_as::<_, CityCluster>(
        r#"
        WITH recent AS (
            SELECT l.city, COUNT(DISTINCT s.id) as signal_count
            FROM signals s
            JOIN locationables la ON la.locatable_type = 'signal' AND la.locatable_id = s.id
            JOIN locations l ON l.id = la.location_id
            WHERE s.created_at > NOW() - INTERVAL '7 days'
              AND l.city IS NOT NULL
            GROUP BY l.city
        ),
        baseline AS (
            SELECT l.city, COUNT(DISTINCT s.id)::float / 4.0 as weekly_avg
            FROM signals s
            JOIN locationables la ON la.locatable_type = 'signal' AND la.locatable_id = s.id
            JOIN locations l ON l.id = la.location_id
            WHERE s.created_at > NOW() - INTERVAL '30 days'
              AND s.created_at <= NOW() - INTERVAL '7 days'
              AND l.city IS NOT NULL
            GROUP BY l.city
        )
        SELECT r.city, r.signal_count
        FROM recent r
        LEFT JOIN baseline b ON b.city = r.city
        WHERE r.signal_count > 5
          AND r.signal_count > COALESCE(b.weekly_avg, 1) * 3
        ORDER BY r.signal_count DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    for cluster in clusters {
        info!(
            city = %cluster.city,
            count = cluster.signal_count,
            "Detected signal cluster"
        );

        // Get the signal IDs in this cluster
        let signal_ids: Vec<Uuid> = sqlx::query_as::<_, SignalId>(
            r#"
            SELECT DISTINCT s.id
            FROM signals s
            JOIN locationables la ON la.locatable_type = 'signal' AND la.locatable_id = s.id
            JOIN locations l ON l.id = la.location_id
            WHERE s.created_at > NOW() - INTERVAL '7 days'
              AND l.city = $1
              AND s.needs_investigation = false
            ORDER BY s.id
            LIMIT 20
            "#,
        )
        .bind(&cluster.city)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();

        if signal_ids.is_empty() {
            continue;
        }

        // Check if existing Finding already covers this cluster via embedding
        let cluster_text = format!(
            "Signal cluster in {} with {} signals in the last 7 days",
            cluster.city, cluster.signal_count
        );
        let already_covered = if let Ok(raw_emb) =
            deps.embedding_service.embed(&cluster_text).await
        {
            let query_vec = Vector::from(raw_emb);
            let similar =
                Embedding::search_similar(query_vec, "finding", 1, 0.2, pool).await?;
            !similar.is_empty()
        } else {
            false
        };

        if already_covered {
            info!(city = %cluster.city, "Cluster already covered by existing finding");
            continue;
        }

        // Flag the first signal for investigation
        let primary_signal_id = signal_ids[0];
        sqlx::query(
            "UPDATE signals SET needs_investigation = true, investigation_status = 'pending', investigation_reason = $1 WHERE id = $2 AND investigation_status = 'pending'",
        )
        .bind(format!(
            "Part of signal cluster in {} ({} signals, 3x+ baseline)",
            cluster.city, cluster.signal_count
        ))
        .bind(primary_signal_id)
        .execute(pool)
        .await?;

        // Trigger investigation
        let trigger = InvestigationTrigger::ClusterDetection {
            signal_ids: signal_ids.clone(),
            city: cluster.city.clone(),
        };

        match run_why_investigation(trigger, deps).await {
            Ok(Some(finding)) => {
                info!(
                    city = %cluster.city,
                    finding_id = %finding.id,
                    "Cluster investigation produced finding"
                );
                triggered_signals.extend(signal_ids);
            }
            Ok(None) => {
                info!(city = %cluster.city, "Cluster investigation did not produce finding");
            }
            Err(e) => {
                tracing::warn!(city = %cluster.city, error = %e, "Cluster investigation failed");
            }
        }
    }

    Ok(triggered_signals)
}
