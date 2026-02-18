use async_trait::async_trait;

use crate::types::{SupervisorStats, ValidationIssue};

/// Pluggable notification backend for the supervisor.
#[async_trait]
pub trait NotifyBackend: Send + Sync {
    /// Send a single validation issue notification.
    async fn send(&self, issue: &ValidationIssue) -> anyhow::Result<()>;

    /// Send a digest summary of a supervisor run.
    async fn send_digest(&self, stats: &SupervisorStats) -> anyhow::Result<()>;
}
