//! Restate durable workflow for the news scanner.
//!
//! Wraps the global (non-regional) `NewsScanner::scan()` in the same
//! Restate pattern used by the other scout workflows.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphStore;

use super::types::{EmptyRequest, NewsScanResult};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "NewsScanWorkflow"]
pub trait NewsScanWorkflow {
    async fn run(req: EmptyRequest) -> Result<NewsScanResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct NewsScanWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl NewsScanWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl NewsScanWorkflow for NewsScanWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<NewsScanResult, HandlerError> {
        ctx.set("status", "Starting news scan...".to_string());

        let deps = self.deps.clone();

        let result = ctx
            .run(|| async {
                scan_news(&deps)
                    .await
                    .map_err(|e| -> HandlerError { e.into() })
            })
            .retry_policy(super::phase_retry_policy())
            .await?;

        ctx.set(
            "status",
            format!(
                "News scan complete: {} articles, {} beacons created",
                result.articles_scanned, result.beacons_created
            ),
        );
        info!(
            articles_scanned = result.articles_scanned,
            beacons_created = result.beacons_created,
            "NewsScanWorkflow complete"
        );

        Ok(result)
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}

/// Run a news scan using shared deps. Usable from both Restate and CLI.
pub async fn scan_news(deps: &ScoutDeps) -> anyhow::Result<NewsScanResult> {
    let archive = super::create_archive(deps);
    let writer = GraphStore::new(deps.graph_client.clone());

    let scanner = crate::news_scanner::NewsScanner::new(
        archive,
        &deps.anthropic_api_key,
        writer,
        deps.daily_budget_cents,
    );

    let (articles_scanned, beacons_created) = scanner.scan().await?;

    Ok(NewsScanResult {
        articles_scanned,
        beacons_created,
    })
}
