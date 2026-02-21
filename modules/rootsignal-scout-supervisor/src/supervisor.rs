use anyhow::Result;
use tracing::{info, warn};

use rootsignal_common::ScoutScope;
use rootsignal_graph::GraphClient;

use crate::checks::{auto_fix, batch_review, echo, report, triage};
use crate::feedback::source_penalty;
use crate::issues::IssueStore;
use crate::notify::backend::NotifyBackend;
use crate::state::SupervisorState;
use crate::types::SupervisorStats;

/// The scout supervisor: validates the graph and feeds back into scout behavior.
pub struct Supervisor {
    client: GraphClient,
    state: SupervisorState,
    issues: IssueStore,
    region: ScoutScope,
    anthropic_api_key: String,
    notifier: Box<dyn NotifyBackend>,
}

impl Supervisor {
    pub fn new(
        client: GraphClient,
        region: ScoutScope,
        anthropic_api_key: String,
        notifier: Box<dyn NotifyBackend>,
    ) -> Self {
        let state = SupervisorState::new(client.clone(), region.name.clone());
        let issues = IssueStore::new(client.clone());
        Self {
            client,
            state,
            issues,
            region,
            anthropic_api_key,
            notifier,
        }
    }

    /// Run the supervisor. Acquires lock, runs checks, releases lock.
    pub async fn run(&self) -> Result<SupervisorStats> {
        // Acquire lock
        let acquired = self.state.acquire_lock().await?;
        if !acquired {
            warn!("Another supervisor is running, exiting");
            return Ok(SupervisorStats::default());
        }

        let result = self.run_inner().await;

        // Always release lock
        if let Err(e) = self.state.release_lock().await {
            warn!(error = %e, "Failed to release supervisor lock");
        }

        result
    }

    async fn run_inner(&self) -> Result<SupervisorStats> {
        let mut stats = SupervisorStats::default();

        // Compute watermark window
        let (from, to) = self.state.watermark_window().await?;
        info!(
            from = %from.format("%Y-%m-%dT%H:%M:%S"),
            to = %to.format("%Y-%m-%dT%H:%M:%S"),
            "Supervisor checking window"
        );

        // Expire stale issues (>30 days old)
        if let Err(e) = self.issues.expire_stale_issues().await {
            warn!(error = %e, "Failed to expire stale issues");
        }

        // Phase 1: Auto-fix checks (deterministic, safe to run anytime)
        stats.auto_fix =
            auto_fix::run_auto_fixes(&self.client, self.region.center_lat, self.region.center_lng)
                .await?;

        // Phase 2: Heuristic triage (cheap graph queries — pre-enrichment for batch review)
        let suspects = triage::triage_suspects(&self.client, &from, &to).await?;

        // Phase 3: Batch review gate (replaces old per-suspect LLM checks)
        match batch_review::review_batch(
            &self.client,
            &self.anthropic_api_key,
            &self.region,
            &suspects,
        )
        .await
        {
            Ok(output) => {
                stats.signals_reviewed = output.signals_reviewed;
                stats.signals_passed = output.signals_passed;
                stats.signals_rejected = output.signals_rejected;

                // Persist validation issues
                for issue in &output.issues {
                    match self.issues.create_if_new(issue).await {
                        Ok(true) => {
                            stats.issues_created += 1;
                            if let Err(e) = self.notifier.send(issue).await {
                                warn!(error = %e, issue_type = %issue.issue_type, "Failed to send notification");
                            }
                        }
                        Ok(false) => {} // Duplicate
                        Err(e) => warn!(error = %e, "Failed to persist ValidationIssue"),
                    }
                }

                // Feedback loop: save report + create GitHub issue if rejections exist
                if output.signals_rejected > 0 {
                    match report::save_report(&self.region.name, &output) {
                        Ok(report_path) => {
                            match report::create_github_issue(&self.region.name, &output, &report_path) {
                                Ok(Some(_url)) => {
                                    stats.github_issue_created = true;
                                }
                                Ok(None) => {} // No analysis or gh unavailable
                                Err(e) => warn!(error = %e, "Failed to create GitHub issue"),
                            }
                        }
                        Err(e) => warn!(error = %e, "Failed to save supervisor report"),
                    }
                }
            }
            Err(e) => {
                // Non-fatal: signals stay staged, reviewed on next run
                warn!(error = %e, "Batch review failed, signals remain staged");
            }
        }

        // Phase 4: Feedback — apply quality penalties to sources with open issues
        if !self.state.is_scout_running().await? {
            match source_penalty::apply_source_penalties(&self.client).await {
                Ok(penalty_stats) => {
                    stats.sources_penalized = penalty_stats.sources_penalized;
                    info!(
                        penalized = penalty_stats.sources_penalized,
                        "Applied source penalties"
                    );
                }
                Err(e) => warn!(error = %e, "Failed to apply source penalties"),
            }

            // Reset penalties for sources whose issues are all resolved
            match source_penalty::reset_resolved_penalties(&self.client).await {
                Ok(count) => stats.sources_reset = count,
                Err(e) => warn!(error = %e, "Failed to reset resolved penalties"),
            }
        } else {
            info!("Scout is running, deferring feedback writes to next run");
        }

        // Phase 5: Echo detection — score stories for single-source flooding
        match echo::detect_echoes(&self.client, 0.7).await {
            Ok(echo_stats) => {
                stats.echoes_flagged = echo_stats.echoes_flagged;
                if echo_stats.stories_scored > 0 {
                    info!(
                        scored = echo_stats.stories_scored,
                        flagged = echo_stats.echoes_flagged,
                        "Echo detection complete"
                    );
                }
            }
            Err(e) => warn!(error = %e, "Failed to run echo detection"),
        }

        // Send digest notification
        if let Err(e) = self.notifier.send_digest(&stats).await {
            warn!(error = %e, "Failed to send digest notification");
        }

        // Update watermark
        self.state.update_last_run(&to).await?;

        info!("Supervisor run complete. {stats}");
        Ok(stats)
    }
}
