use async_trait::async_trait;
use serde_json::json;
use tracing::warn;

use super::backend::NotifyBackend;
use crate::types::{Severity, SupervisorStats, ValidationIssue};

/// Slack incoming webhook notification backend.
pub struct SlackWebhook {
    webhook_url: String,
    http: reqwest::Client,
}

impl SlackWebhook {
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            http: reqwest::Client::new(),
        }
    }

    fn severity_emoji(severity: &Severity) -> &'static str {
        match severity {
            Severity::Info => ":information_source:",
            Severity::Warning => ":warning:",
            Severity::Error => ":rotating_light:",
        }
    }

    async fn post(&self, payload: serde_json::Value) -> anyhow::Result<()> {
        let resp = self
            .http
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "Slack webhook returned non-success");
            anyhow::bail!("Slack webhook returned {status}");
        }

        Ok(())
    }
}

#[async_trait]
impl NotifyBackend for SlackWebhook {
    async fn send(&self, issue: &ValidationIssue) -> anyhow::Result<()> {
        let emoji = Self::severity_emoji(&issue.severity);
        let text = format!(
            "{emoji} *Scout Supervisor — {}*\n\
             *Type:* {}\n\
             *Target:* {} `{}`\n\
             *Region:* {}\n\n\
             {}\n\n\
             *Suggested action:* {}",
            issue.severity,
            issue.issue_type,
            issue.target_label,
            issue.target_id,
            issue.region,
            issue.description,
            issue.suggested_action,
        );

        let payload = json!({
            "text": text,
            "unfurl_links": false,
        });

        self.post(payload).await
    }

    async fn send_digest(&self, stats: &SupervisorStats) -> anyhow::Result<()> {
        let auto = &stats.auto_fix;
        let has_fixes = auto.orphaned_citations_deleted > 0
            || auto.orphaned_edges_deleted > 0
            || auto.actors_merged > 0
            || auto.empty_signals_deleted > 0
            || auto.fake_coords_nulled > 0;

        if !has_fixes && stats.issues_created == 0 {
            // Nothing to report
            return Ok(());
        }

        let mut lines = vec![":broom: *Scout Supervisor Run Complete*".to_string()];

        if has_fixes {
            lines.push("*Auto-fixes applied:*".to_string());
            if auto.orphaned_citations_deleted > 0 {
                lines.push(format!(
                    "  - Orphaned citations deleted: {}",
                    auto.orphaned_citations_deleted
                ));
            }
            if auto.orphaned_edges_deleted > 0 {
                lines.push(format!(
                    "  - Orphaned actors deleted: {}",
                    auto.orphaned_edges_deleted
                ));
            }
            if auto.actors_merged > 0 {
                lines.push(format!(
                    "  - Duplicate actors merged: {}",
                    auto.actors_merged
                ));
            }
            if auto.empty_signals_deleted > 0 {
                lines.push(format!(
                    "  - Empty signals deleted: {}",
                    auto.empty_signals_deleted
                ));
            }
            if auto.fake_coords_nulled > 0 {
                lines.push(format!(
                    "  - Fake coordinates nulled: {}",
                    auto.fake_coords_nulled
                ));
            }
        }

        if stats.issues_created > 0 {
            lines.push(format!("*New issues flagged:* {}", stats.issues_created));
        }

        lines.push(format!(
            "_Reviewed {} signals (passed={}, rejected={})_",
            stats.signals_reviewed, stats.signals_passed, stats.signals_rejected
        ));

        let payload = json!({
            "text": lines.join("\n"),
            "unfurl_links": false,
        });

        self.post(payload).await
    }
}
