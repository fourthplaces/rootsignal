use async_trait::async_trait;
use tracing::warn;

use super::backend::NotifyBackend;
use super::slack::SlackWebhook;
use crate::types::{SupervisorStats, ValidationIssue};

/// Routes notifications to different backends based on configuration.
/// Supports separate Slack channels for auto-fix digests vs flagged issues.
pub struct NotifyRouter {
    /// Default backend for flagged issues.
    flags_backend: Box<dyn NotifyBackend>,
    /// Backend for auto-fix digests (may be same or different channel).
    digest_backend: Box<dyn NotifyBackend>,
}

impl NotifyRouter {
    /// Build a router from environment configuration.
    ///
    /// Env vars:
    /// - `SLACK_WEBHOOK_URL` — default Slack webhook
    /// - `SLACK_WEBHOOK_URL_FLAGS` — override for flagged issues (optional)
    /// - `SLACK_WEBHOOK_URL_DIGEST` — override for digest summaries (optional)
    pub fn from_env() -> Option<Self> {
        let default_url = std::env::var("SLACK_WEBHOOK_URL").ok()?;

        let flags_url =
            std::env::var("SLACK_WEBHOOK_URL_FLAGS").unwrap_or_else(|_| default_url.clone());
        let digest_url =
            std::env::var("SLACK_WEBHOOK_URL_DIGEST").unwrap_or_else(|_| default_url.clone());

        Some(Self {
            flags_backend: Box::new(SlackWebhook::new(flags_url)),
            digest_backend: Box::new(SlackWebhook::new(digest_url)),
        })
    }
}

#[async_trait]
impl NotifyBackend for NotifyRouter {
    async fn send(&self, issue: &ValidationIssue) -> anyhow::Result<()> {
        if let Err(e) = self.flags_backend.send(issue).await {
            warn!(error = %e, issue_type = %issue.issue_type, "Failed to send flag notification");
        }
        Ok(())
    }

    async fn send_digest(&self, stats: &SupervisorStats) -> anyhow::Result<()> {
        if let Err(e) = self.digest_backend.send_digest(stats).await {
            warn!(error = %e, "Failed to send digest notification");
        }
        Ok(())
    }
}
