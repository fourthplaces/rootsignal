use anyhow::Result;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::findings::models::connection::Connection;
use crate::findings::models::finding::Finding;

#[derive(Debug, sqlx::FromRow)]
struct FindingRow {
    id: Uuid,
    status: String,
    validation_status: Option<String>,
}

/// Run lifecycle transitions for all findings.
/// Call this periodically (e.g., daily via Restate scheduled service).
pub async fn update_finding_lifecycles(pool: &PgPool) -> Result<()> {
    let findings = sqlx::query_as::<_, FindingRow>(
        "SELECT id, status, validation_status FROM findings WHERE status != 'resolved'",
    )
    .fetch_all(pool)
    .await?;

    for f in findings {
        let new_status = evaluate_transition(&f, pool).await?;
        if let Some(status) = new_status {
            Finding::update_status(f.id, &status, pool).await?;
            info!(finding_id = %f.id, from = %f.status, to = %status, "Finding status transition");
        }

        // Update signal velocity (rolling 7-day average)
        let recent_connections = Connection::count_recent("finding", f.id, 7, pool).await?;
        let velocity = recent_connections as f32 / 7.0;
        Finding::update_velocity(f.id, velocity, pool).await?;
    }

    Ok(())
}

async fn evaluate_transition(f: &FindingRow, pool: &PgPool) -> Result<Option<String>> {
    let connection_count = Finding::connection_count(f.id, pool).await?;
    let latest = Connection::latest_connection_at("finding", f.id, pool).await?;

    let days_since_last = latest
        .map(|t| (chrono::Utc::now() - t).num_days())
        .unwrap_or(999);

    match f.status.as_str() {
        "emerging" => {
            // emerging → active: validated AND 3+ connections
            if f.validation_status.as_deref() == Some("validated") && connection_count >= 3 {
                return Ok(Some("active".to_string()));
            }
        }
        "active" => {
            // active → declining: no new connections in 14+ days
            if days_since_last >= 14 {
                return Ok(Some("declining".to_string()));
            }
        }
        "declining" => {
            // declining → resolved: no new connections in 30+ days
            if days_since_last >= 30 {
                return Ok(Some("resolved".to_string()));
            }
            // declining → active: new connections appeared
            if days_since_last < 7 {
                return Ok(Some("active".to_string()));
            }
        }
        _ => {}
    }

    Ok(None)
}
