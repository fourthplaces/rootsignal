//! Typed client for invoking Restate workflows via the HTTP ingress.
//!
//! The Restate Rust SDK doesn't ship an ingress client, so we wrap reqwest
//! with typed methods for each workflow we need to call from the API server.

use reqwest::Client;
use rootsignal_common::ScoutScope;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum RestateError {
    #[error("Restate ingress error (HTTP {status}): {body}")]
    Ingress { status: u16, body: String },

    #[error("Restate unreachable: {0}")]
    Unreachable(#[from] reqwest::Error),
}

/// Individual scout workflow phases that can be run independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoutPhase {
    Bootstrap,
    Scrape,
    Synthesis,
    SituationWeaver,
    Supervisor,
}

impl ScoutPhase {
    /// Restate workflow name for this phase.
    pub fn workflow_name(&self) -> &'static str {
        match self {
            Self::Bootstrap => "BootstrapWorkflow",
            Self::Scrape => "ScrapeWorkflow",
            Self::Synthesis => "SynthesisWorkflow",
            Self::SituationWeaver => "SituationWeaverWorkflow",
            Self::Supervisor => "SupervisorWorkflow",
        }
    }
}

/// Client for dispatching Restate workflows via the HTTP ingress.
///
/// Reuses a single `reqwest::Client` for connection pooling.
#[derive(Clone)]
pub struct RestateClient {
    http: Client,
    ingress_url: String,
}

impl RestateClient {
    pub fn new(ingress_url: String) -> Self {
        Self {
            http: Client::new(),
            ingress_url,
        }
    }

    /// Start a `FullScoutRunWorkflow` for the given task.
    /// Restate key = task_id (UUID, inherently unique, one-shot).
    pub async fn run_scout(&self, task_id: &str, scope: &ScoutScope) -> Result<(), RestateError> {
        let url = format!("{}/FullScoutRunWorkflow/{task_id}/run", self.ingress_url);
        info!(url = url.as_str(), task_id, "Dispatching scout via Restate");

        let body = serde_json::json!({ "task_id": task_id, "scope": scope });
        let resp = self.http.post(&url).json(&body).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }

    /// Dispatch an individual scout workflow phase via Restate ingress.
    /// Restate key = task_id (different workflow types have separate key spaces).
    pub async fn run_phase(
        &self,
        phase: ScoutPhase,
        task_id: &str,
        scope: &ScoutScope,
    ) -> Result<(), RestateError> {
        let workflow_name = phase.workflow_name();
        let key = format!("{task_id}-{}", chrono::Utc::now().timestamp());
        let url = format!("{}/{workflow_name}/{key}/run", self.ingress_url);
        info!(url = url.as_str(), phase = ?phase, task_id, "Dispatching individual phase via Restate");

        let body = match phase {
            ScoutPhase::Synthesis | ScoutPhase::SituationWeaver => {
                serde_json::json!({ "task_id": task_id, "scope": scope, "spent_cents": 0u64 })
            }
            _ => serde_json::json!({ "task_id": task_id, "scope": scope }),
        };

        let resp = self.http.post(&url).json(&body).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }

    /// Start a `NewsScanWorkflow` (global, no region).
    pub async fn run_news_scan(&self) -> Result<(), RestateError> {
        let key = format!("news-{}", chrono::Utc::now().timestamp());
        let url = format!("{}/NewsScanWorkflow/{key}/run", self.ingress_url);
        info!(url = url.as_str(), "Dispatching news scan via Restate");

        let body = serde_json::json!({});
        let resp = self.http.post(&url).json(&body).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }

    /// Cancel a running `FullScoutRunWorkflow`.
    pub async fn cancel_scout(&self, task_id: &str) -> Result<(), RestateError> {
        let url = format!(
            "{}/restate/workflow/FullScoutRunWorkflow/{task_id}/cancel",
            self.ingress_url
        );
        info!(url = url.as_str(), task_id, "Cancelling scout via Restate");

        let resp = self.http.delete(&url).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }
}
