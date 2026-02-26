//! Typed client for invoking Restate workflows via the HTTP ingress.
//!
//! The Restate Rust SDK doesn't ship an ingress client, so we wrap reqwest
//! with typed methods for each workflow we need to call from the API server.

use reqwest::Client;
use rootsignal_common::ScoutScope;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum RestateError {
    #[error("Restate ingress error (HTTP {status}): {body}")]
    Ingress { status: u16, body: String },

    #[error("Restate unreachable: {0}")]
    Unreachable(#[from] reqwest::Error),
}

/// Live invocation status from the Restate Admin API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestateInvocationStatus {
    Pending,
    Scheduled,
    Ready,
    Running,
    Paused,
    BackingOff,
    Suspended,
    Completed,
    Unknown(String),
}

impl RestateInvocationStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "scheduled" => Self::Scheduled,
            "ready" => Self::Ready,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "backing-off" => Self::BackingOff,
            "suspended" => Self::Suspended,
            "completed" => Self::Completed,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Scheduled => "scheduled",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::BackingOff => "backing-off",
            Self::Suspended => "suspended",
            Self::Completed => "completed",
            Self::Unknown(s) => s,
        }
    }
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
    admin_url: Option<String>,
}

impl RestateClient {
    pub fn new(ingress_url: String, admin_url: Option<String>) -> Self {
        Self {
            http: Client::new(),
            ingress_url,
            admin_url,
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

    /// Start a `ScrapeUrlWorkflow` for a single URL.
    pub async fn scrape_url(&self, url: &str) -> Result<(), RestateError> {
        let key = format!("url-{}", chrono::Utc::now().timestamp_millis());
        let ingress_url = format!("{}/ScrapeUrlWorkflow/{key}/run", self.ingress_url);
        info!(url = url, ingress_url = ingress_url.as_str(), "Dispatching single URL scrape via Restate");

        let body = serde_json::json!({ "url": url });
        let resp = self.http.post(&ingress_url).json(&body).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }

    /// Cancel a running workflow by name and key.
    pub async fn cancel_workflow(
        &self,
        workflow_name: &str,
        key: &str,
    ) -> Result<(), RestateError> {
        let url = format!(
            "{}/restate/workflow/{workflow_name}/{key}/cancel",
            self.ingress_url
        );
        info!(url = url.as_str(), workflow_name, key, "Cancelling workflow via Restate");

        let resp = self.http.delete(&url).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }

    /// Seed the SignalReaper object with an initial fire-and-forget invocation.
    ///
    /// Uses Restate's `/send` semantics so we don't block on the result.
    /// The reaper self-reschedules after each run, so this only needs to
    /// fire once. Restate deduplicates if the object is already running.
    pub async fn seed_reaper(&self) -> Result<(), RestateError> {
        let url = format!("{}/SignalReaper/global/run/send", self.ingress_url);
        info!(url = url.as_str(), "Seeding signal reaper via Restate");

        let resp = self.http.post(&url).json(&serde_json::json!({})).send().await?;

        if resp.status().is_success() {
            info!("Signal reaper seeded successfully");
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(RestateError::Ingress { status, body })
        }
    }

    /// Query the Restate Admin API for the live invocation status of a workflow.
    /// Returns `None` if the admin URL is not configured or no invocation is found.
    pub async fn invocation_status(
        &self,
        workflow_name: &str,
        key: &str,
    ) -> Result<Option<RestateInvocationStatus>, RestateError> {
        let admin_url = match &self.admin_url {
            Some(url) => url,
            None => return Ok(None),
        };

        let query = format!(
            "SELECT status FROM sys_invocation WHERE target_service_name = '{}' AND target_service_key = '{}' ORDER BY created_at DESC LIMIT 1",
            workflow_name, key
        );

        let url = format!("{admin_url}/query");
        let resp = self
            .http
            .post(&url)
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        let resp_status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !resp_status.is_success() {
            warn!(status = resp_status.as_u16(), body = %body, "Restate admin query failed");
            return Ok(None);
        }

        // Parse the response as generic JSON so we can handle whatever shape Restate returns.
        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, body = %body, "Failed to parse Restate admin query response");
                return Ok(None);
            }
        };

        // Try rows as array-of-arrays: {"rows": [["running"]]}
        if let Some(rows) = json.get("rows").and_then(|v| v.as_array()) {
            if let Some(first_row) = rows.first() {
                let status_val = if let Some(arr) = first_row.as_array() {
                    arr.first()
                } else if let Some(obj) = first_row.as_object() {
                    // rows as array-of-objects: {"rows": [{"status": "running"}]}
                    obj.get("status")
                } else {
                    None
                };
                if let Some(s) = status_val.and_then(|v| v.as_str()) {
                    return Ok(Some(RestateInvocationStatus::from_str(s)));
                }
            }
            return Ok(None);
        }

        warn!(body = %body, "Unexpected Restate admin query response shape");
        Ok(None)
    }
}
