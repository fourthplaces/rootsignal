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

#[derive(Default)]
pub struct WorkflowMutation;

#[Object]
impl WorkflowMutation {
    /// Trigger a scrape workflow for a specific source.
    async fn trigger_scrape(&self, ctx: &Context<'_>, source_id: Uuid) -> Result<WorkflowTriggerResult> {
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps.config.restate_admin_url.as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080"); // Restate ingress port

        let url = format!("{}/ScrapeWorkflow/{}/run/send", ingress_url, source_id);
        let response = deps.http_client.post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| Error::new(format!("Failed to trigger scrape: {e}")))?;

        let status = if response.status().is_success() { "triggered" } else { "failed" };

        Ok(WorkflowTriggerResult {
            workflow_id: source_id.to_string(),
            status: status.to_string(),
        })
    }

    /// Trigger a full scrape cycle for all due sources.
    async fn trigger_scrape_cycle(&self, ctx: &Context<'_>) -> Result<WorkflowTriggerResult> {
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps.config.restate_admin_url.as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080");

        let url = format!("{}/SchedulerService/scheduler/startCycle/send", ingress_url);
        let response = deps.http_client.post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| Error::new(format!("Failed to trigger scrape cycle: {e}")))?;

        let status = if response.status().is_success() { "triggered" } else { "failed" };

        Ok(WorkflowTriggerResult {
            workflow_id: "scrape-cycle".to_string(),
            status: status.to_string(),
        })
    }

    /// Trigger extraction for a specific snapshot.
    async fn trigger_extraction(&self, ctx: &Context<'_>, snapshot_id: Uuid) -> Result<WorkflowTriggerResult> {
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps.config.restate_admin_url.as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080");

        let url = format!("{}/ExtractWorkflow/{}/run/send", ingress_url, snapshot_id);
        let response = deps.http_client.post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| Error::new(format!("Failed to trigger extraction: {e}")))?;

        let status = if response.status().is_success() { "triggered" } else { "failed" };

        Ok(WorkflowTriggerResult {
            workflow_id: snapshot_id.to_string(),
            status: status.to_string(),
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
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps.config.restate_admin_url.as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080");

        let workflow_id = format!("{}-{}", translatable_type, translatable_id);
        let url = format!("{}/TranslateWorkflow/{}/run/send", ingress_url, workflow_id);
        let response = deps.http_client.post(&url)
            .json(&serde_json::json!({
                "translatable_type": translatable_type,
                "translatable_id": translatable_id.to_string(),
                "target_locale": locale,
            }))
            .send()
            .await
            .map_err(|e| Error::new(format!("Failed to trigger translation: {e}")))?;

        let status = if response.status().is_success() { "triggered" } else { "failed" };

        Ok(WorkflowTriggerResult {
            workflow_id,
            status: status.to_string(),
        })
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
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let restate_admin_url = deps.config.restate_admin_url.as_ref()
            .ok_or_else(|| Error::new("Restate not configured"))?;
        let ingress_url = restate_admin_url.replace(":9070", ":8080");

        let url = format!("{}/{}/{}/get_status", ingress_url, workflow_type, workflow_id);
        let response = deps.http_client.get(&url)
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
