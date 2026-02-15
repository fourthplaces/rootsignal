use async_graphql::*;
use std::sync::Arc;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use rootsignal_core::ServerDeps;

#[derive(SimpleObject)]
pub struct WorkflowTriggerResult {
    pub workflow_id: String,
    pub status: String,
}

/// Shared helper to trigger a Restate workflow and surface errors properly.
pub(crate) async fn trigger_restate_workflow(
    deps: &ServerDeps,
    workflow: &str,
    key: &str,
    body: serde_json::Value,
) -> Result<WorkflowTriggerResult> {
    let restate_admin_url = deps
        .config
        .restate_admin_url
        .as_ref()
        .ok_or_else(|| Error::new("Restate not configured"))?;
    let ingress_url = restate_admin_url.replace(":9070", ":8080");

    let url = format!("{}/{}/{}/run/send", ingress_url, workflow, key);
    tracing::info!(url = %url, "Triggering Restate workflow");

    let response = deps
        .http_client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::new(format!("Restate request failed: {e}")))?;

    let status_code = response.status();
    let response_body = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable body>".to_string());

    tracing::info!(
        workflow = workflow,
        key = key,
        status = %status_code,
        body = %response_body,
        "Restate response"
    );

    if status_code.is_success() {
        Ok(WorkflowTriggerResult {
            workflow_id: key.to_string(),
            status: "triggered".to_string(),
        })
    } else {
        Err(Error::new(format!(
            "Restate error ({}): {}",
            status_code,
            &response_body[..response_body.len().min(500)]
        )))
    }
}

#[derive(Default)]
pub struct WorkflowMutation;

#[Object]
impl WorkflowMutation {
    /// Trigger a scrape workflow for a specific source.
    async fn trigger_scrape(
        &self,
        ctx: &Context<'_>,
        source_id: Uuid,
    ) -> Result<WorkflowTriggerResult> {
        tracing::info!(source_id = %source_id, "graphql.trigger_scrape");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;
        let key = format!("{}-{}", source_id, chrono::Utc::now().timestamp());
        trigger_restate_workflow(deps, "ScrapeWorkflow", &key, serde_json::json!({
            "source_id": source_id.to_string(),
        })).await
    }

    /// Trigger a full scrape cycle for all due sources.
    async fn trigger_scrape_cycle(&self, ctx: &Context<'_>) -> Result<WorkflowTriggerResult> {
        tracing::info!("graphql.trigger_scrape_cycle");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps
            .config
            .restate_admin_url
            .as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080");

        let url = format!("{}/SchedulerService/scheduler/startCycle/send", ingress_url);
        let response = deps
            .http_client
            .post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| Error::new(format!("Restate request failed: {e}")))?;

        if response.status().is_success() {
            Ok(WorkflowTriggerResult {
                workflow_id: "scrape-cycle".to_string(),
                status: "triggered".to_string(),
            })
        } else {
            let status_code = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            tracing::error!(
                status = %status_code,
                body = %body,
                "Restate returned error for scrape cycle"
            );
            Err(Error::new(format!(
                "Restate error ({}): {}",
                status_code,
                &body[..body.len().min(500)]
            )))
        }
    }

    /// Trigger extraction for a specific snapshot.
    async fn trigger_extraction(
        &self,
        ctx: &Context<'_>,
        snapshot_id: Uuid,
    ) -> Result<WorkflowTriggerResult> {
        tracing::info!(snapshot_id = %snapshot_id, "graphql.trigger_extraction");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;
        let key = format!("{}-{}", snapshot_id, chrono::Utc::now().timestamp());
        trigger_restate_workflow(deps, "ExtractWorkflow", &key, serde_json::json!({
            "snapshot_ids": [snapshot_id.to_string()],
        })).await
    }

    /// Trigger source qualification — sample scrape + AI evaluation.
    async fn trigger_qualification(
        &self,
        ctx: &Context<'_>,
        source_id: Uuid,
    ) -> Result<WorkflowTriggerResult> {
        tracing::info!(source_id = %source_id, "graphql.trigger_qualification");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;
        let key = format!("{}-{}", source_id, chrono::Utc::now().timestamp());
        trigger_restate_workflow(deps, "QualifyWorkflow", &key, serde_json::json!({
            "source_id": source_id.to_string(),
        })).await
    }

    /// Trigger qualification for all sources with pending status.
    async fn trigger_qualify_pending(&self, ctx: &Context<'_>) -> Result<WorkflowTriggerResult> {
        tracing::info!("graphql.trigger_qualify_pending");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let pending = rootsignal_domains::scraping::Source::find_pending_qualification(pool)
            .await
            .map_err(|e| Error::new(format!("database error: {e}")))?;

        let count = pending.len();
        tracing::info!(count = count, "Qualifying pending sources");

        for source in &pending {
            let key = format!("{}-{}", source.id, chrono::Utc::now().timestamp());
            if let Err(e) = trigger_restate_workflow(
                deps,
                "QualifyWorkflow",
                &key,
                serde_json::json!({ "source_id": source.id.to_string() }),
            )
            .await
            {
                tracing::warn!(source_id = %source.id, error = ?e, "Failed to trigger qualification");
            }
        }

        Ok(WorkflowTriggerResult {
            workflow_id: format!("qualify-pending-{}", count),
            status: format!("triggered {} qualifications", count),
        })
    }

    /// Trigger translation for a specific record.
    async fn trigger_translation(
        &self,
        ctx: &Context<'_>,
        translatable_type: String,
        translatable_id: Uuid,
        locale: String,
    ) -> Result<WorkflowTriggerResult> {
        tracing::info!(translatable_type = %translatable_type, translatable_id = %translatable_id, locale = %locale, "graphql.trigger_translation");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;
        let key = format!("{}-{}-{}", translatable_type, translatable_id, chrono::Utc::now().timestamp());
        trigger_restate_workflow(deps, "TranslateWorkflow", &key, serde_json::json!({
            "translatable_type": translatable_type,
            "translatable_id": translatable_id.to_string(),
            "source_locale": locale,
        })).await
    }
}

#[derive(Default)]
pub struct WorkflowQuery;

#[Object]
impl WorkflowQuery {
    /// Check the status of a running workflow.
    async fn workflow_status(
        &self,
        ctx: &Context<'_>,
        workflow_type: String,
        workflow_id: String,
    ) -> Result<String> {
        tracing::info!(workflow_type = %workflow_type, workflow_id = %workflow_id, "graphql.workflow_status");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps
            .config
            .restate_admin_url
            .as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080");

        let url = format!(
            "{}/{}/{}/get_status",
            ingress_url, workflow_type, workflow_id
        );
        let response = deps
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::new(format!("Failed to get workflow status: {e}")))?;

        if response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            Ok(body)
        } else {
            Ok("unknown".to_string())
        }
    }
}
