use async_graphql::*;
use std::sync::Arc;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::workflows::{trigger_restate_workflow, WorkflowTriggerResult};
use rootsignal_core::ServerDeps;
use rootsignal_domains::findings::{Connection, Finding};

use super::types::*;

#[derive(Default)]
pub struct FindingMutation;

#[Object]
impl FindingMutation {
    /// Manually trigger investigation for a signal.
    async fn trigger_investigation(
        &self,
        ctx: &Context<'_>,
        signal_id: Uuid,
    ) -> Result<WorkflowTriggerResult> {
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let pool = deps.pool();

        // Guard: prevent re-triggering while already in progress
        let current_status = sqlx::query_scalar::<_, Option<String>>(
            "SELECT investigation_status FROM signals WHERE id = $1",
        )
        .bind(signal_id)
        .fetch_one(pool)
        .await
        .map_err(|e| Error::new(format!("Signal not found: {e}")))?;

        if current_status.as_deref() == Some("in_progress") {
            return Err(Error::new("Investigation already in progress for this signal"));
        }

        // Mark the signal for investigation
        sqlx::query(
            "UPDATE signals SET needs_investigation = true, investigation_status = 'pending', investigation_reason = 'Manual trigger' WHERE id = $1",
        )
        .bind(signal_id)
        .execute(pool)
        .await
        .map_err(|e| Error::new(format!("Failed to flag signal: {e}")))?;

        let key = format!("why-{}-{}", signal_id, chrono::Utc::now().timestamp());
        trigger_restate_workflow(
            deps,
            "WhyInvestigationWorkflow",
            &key,
            serde_json::json!({ "signal_id": signal_id.to_string() }),
        )
        .await
    }

    /// Trigger investigations for all pending signals (up to concurrency limit).
    async fn run_pending_investigations(
        &self,
        ctx: &Context<'_>,
    ) -> Result<i32> {
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;
        let pool = deps.pool();

        // Check how many are already running
        let in_progress: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM signals WHERE investigation_status = 'in_progress'",
        )
        .fetch_one(pool)
        .await
        .map_err(|e| Error::new(format!("Query failed: {e}")))?;

        let slots = (5 - in_progress).max(0) as i64;
        if slots == 0 {
            return Ok(0);
        }

        // Find signals that need investigation but aren't yet running
        let pending_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM signals WHERE needs_investigation = true AND (investigation_status IS NULL OR investigation_status = 'pending') ORDER BY created_at DESC LIMIT $1",
        )
        .bind(slots)
        .fetch_all(pool)
        .await
        .map_err(|e| Error::new(format!("Query failed: {e}")))?;

        let mut triggered = 0;
        for signal_id in pending_ids {
            sqlx::query(
                "UPDATE signals SET investigation_status = 'pending' WHERE id = $1",
            )
            .bind(signal_id)
            .execute(pool)
            .await?;

            let key = format!("why-{}-{}", signal_id, chrono::Utc::now().timestamp());
            if trigger_restate_workflow(
                deps,
                "WhyInvestigationWorkflow",
                &key,
                serde_json::json!({ "signal_id": signal_id.to_string() }),
            )
            .await
            .is_ok()
            {
                triggered += 1;
            }
        }

        Ok(triggered)
    }

    /// Admin override: update finding status.
    async fn update_finding_status(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        status: FindingStatus,
    ) -> Result<GqlFinding> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let finding = Finding::update_status(id, status.as_str(), pool).await?;
        Ok(GqlFinding::from(finding))
    }

    /// Manually create a connection between nodes.
    async fn create_connection(
        &self,
        ctx: &Context<'_>,
        from_type: String,
        from_id: Uuid,
        to_type: String,
        to_id: Uuid,
        role: String,
        causal_quote: Option<String>,
    ) -> Result<GqlConnection> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let conn = Connection::create(
            &from_type,
            from_id,
            &to_type,
            to_id,
            &role,
            causal_quote.as_deref(),
            Some(1.0), // manual connections are high confidence
            pool,
        )
        .await?;
        Ok(GqlConnection::from(conn))
    }
}
