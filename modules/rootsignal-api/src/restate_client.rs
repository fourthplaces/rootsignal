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
    ActorDiscovery,
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
            Self::ActorDiscovery => "ActorDiscoveryWorkflow",
            Self::Scrape => "ScrapeWorkflow",
            Self::Synthesis => "SynthesisWorkflow",
            Self::SituationWeaver => "SituationWeaverWorkflow",
            Self::Supervisor => "SupervisorWorkflow",
        }
    }

    /// The `running_*` status string for this phase.
    pub fn running_status(&self) -> &'static str {
        match self {
            Self::Bootstrap => "running_bootstrap",
            Self::ActorDiscovery => "running_actor_discovery",
            Self::Scrape => "running_scrape",
            Self::Synthesis => "running_synthesis",
            Self::SituationWeaver => "running_situation_weaver",
            Self::Supervisor => "running_supervisor",
        }
    }

    /// Status values from which this phase is allowed to run.
    pub fn allowed_from_statuses(&self) -> &'static [&'static str] {
        match self {
            Self::Bootstrap => &[
                "idle", "bootstrap_complete", "actor_discovery_complete",
                "scrape_complete", "synthesis_complete", "situation_weaver_complete", "complete",
            ],
            Self::ActorDiscovery => &[
                "bootstrap_complete", "actor_discovery_complete", "scrape_complete",
                "synthesis_complete", "situation_weaver_complete", "complete",
            ],
            Self::Scrape => &[
                "actor_discovery_complete", "scrape_complete", "synthesis_complete",
                "situation_weaver_complete", "complete",
            ],
            Self::Synthesis => &[
                "scrape_complete", "synthesis_complete",
                "situation_weaver_complete", "complete",
            ],
            Self::SituationWeaver => &[
                "synthesis_complete", "situation_weaver_complete", "complete",
            ],
            Self::Supervisor => &[
                "situation_weaver_complete", "complete",
            ],
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

    /// Start a `FullScoutRunWorkflow` for the given region slug.
    pub async fn run_scout(&self, slug: &str, scope: &ScoutScope) -> Result<(), RestateError> {
        let url = format!("{}/FullScoutRunWorkflow/{slug}/run", self.ingress_url);
        info!(url = url.as_str(), "Dispatching scout via Restate");

        let body = serde_json::json!({ "scope": scope });
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
    /// Uses a timestamped key to avoid collision with Full Run sub-workflows.
    pub async fn run_phase(
        &self,
        phase: ScoutPhase,
        slug: &str,
        scope: &ScoutScope,
    ) -> Result<(), RestateError> {
        let workflow_name = phase.workflow_name();
        let key = format!("{slug}-{}", chrono::Utc::now().timestamp());
        let url = format!("{}/{workflow_name}/{key}/run", self.ingress_url);
        info!(url = url.as_str(), phase = ?phase, "Dispatching individual phase via Restate");

        let body = match phase {
            ScoutPhase::Synthesis | ScoutPhase::SituationWeaver => {
                serde_json::json!({ "scope": scope, "spent_cents": 0u64 })
            }
            _ => serde_json::json!({ "scope": scope }),
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

    /// Cancel a running `FullScoutRunWorkflow`.
    pub async fn cancel_scout(&self, slug: &str) -> Result<(), RestateError> {
        let url = format!(
            "{}/restate/workflow/FullScoutRunWorkflow/{slug}/cancel",
            self.ingress_url
        );
        info!(url = url.as_str(), "Cancelling scout via Restate");

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
