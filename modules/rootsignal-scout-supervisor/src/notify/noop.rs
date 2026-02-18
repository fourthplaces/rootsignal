use async_trait::async_trait;

use crate::types::{SupervisorStats, ValidationIssue};
use super::backend::NotifyBackend;

/// No-op notification backend for testing.
pub struct NoopBackend;

#[async_trait]
impl NotifyBackend for NoopBackend {
    async fn send(&self, _issue: &ValidationIssue) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send_digest(&self, _stats: &SupervisorStats) -> anyhow::Result<()> {
        Ok(())
    }
}
