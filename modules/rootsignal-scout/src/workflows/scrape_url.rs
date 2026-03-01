//! Restate durable workflow for scraping a single URL.
//!
//! No task or scheduling needed — uses a global scope and delegates to
//! `scrape_single_url` for the actual fetch + extraction + store cycle.

use std::collections::HashSet;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_common::ScoutScope;

use super::types::{EmptyRequest, ScrapeUrlRequest, ScrapeUrlResult};
use super::ScoutDeps;
use crate::domains::scrape::activities::scrape_phase::scrape_single_url;
use crate::workflows::scrape::build_phase;

#[restate_sdk::workflow]
#[name = "ScrapeUrlWorkflow"]
pub trait ScrapeUrlWorkflow {
    async fn run(req: ScrapeUrlRequest) -> Result<ScrapeUrlResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ScrapeUrlWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl ScrapeUrlWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl ScrapeUrlWorkflow for ScrapeUrlWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: ScrapeUrlRequest,
    ) -> Result<ScrapeUrlResult, HandlerError> {
        let url = req.url.clone();
        ctx.set("status", format!("Scraping {url}..."));

        let scope = ScoutScope {
            name: "Global".to_string(),
            center_lat: 0.0,
            center_lng: 0.0,
            radius_km: 40_000.0,
        };
        let run_id = uuid::Uuid::new_v4().to_string();
        let phase = build_phase(&self.deps, &scope, &run_id);

        let canonical_key = rootsignal_common::canonical_value(&url);
        let known_urls: HashSet<String> = HashSet::new();
        let run_log = crate::infra::run_log::RunLogger::new(
            run_id.clone(), scope.name.clone(), self.deps.pg_pool.clone(),
        ).await;

        let outcome = ctx
            .run(|| {
                let phase = phase.clone();
                let url = url.clone();
                let ck = canonical_key.clone();
                let known = known_urls.clone();
                let log_clone = run_log.clone();
                async move {
                    let result = scrape_single_url(
                        &phase, &url, &ck, None, None, None, &known, &log_clone,
                    )
                    .await;
                    Ok(result)
                }
            })
            .await?;

        let status_msg = format!(
            "Scrape complete: {} signals stored",
            outcome.signals_stored,
        );
        ctx.set("status", status_msg.clone());
        info!(url = req.url.as_str(), signals = outcome.signals_stored, "ScrapeUrlWorkflow complete");

        Ok(ScrapeUrlResult {
            signals_stored: outcome.signals_stored,
            status: status_msg,
        })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}
