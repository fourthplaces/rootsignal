use anyhow::Result;
use tracing::{info, warn};

use rootsignal_common::CityNode;
use rootsignal_graph::GraphClient;

use crate::budget::BudgetTracker;
use crate::checks::{auto_fix, llm, triage};
use crate::issues::IssueStore;
use crate::notify::backend::NotifyBackend;
use crate::state::SupervisorState;
use crate::types::SupervisorStats;

/// Default max LLM checks per run.
const DEFAULT_MAX_LLM_CHECKS: u64 = 50;

/// The scout supervisor: validates the graph and feeds back into scout behavior.
pub struct Supervisor {
    client: GraphClient,
    state: SupervisorState,
    issues: IssueStore,
    city: CityNode,
    anthropic_api_key: String,
    notifier: Box<dyn NotifyBackend>,
    max_llm_checks: u64,
}

impl Supervisor {
    pub fn new(
        client: GraphClient,
        city: CityNode,
        anthropic_api_key: String,
        notifier: Box<dyn NotifyBackend>,
    ) -> Self {
        let state = SupervisorState::new(client.clone(), city.slug.clone());
        let issues = IssueStore::new(client.clone());
        Self {
            client,
            state,
            issues,
            city,
            anthropic_api_key,
            notifier,
            max_llm_checks: DEFAULT_MAX_LLM_CHECKS,
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
        stats.auto_fix = auto_fix::run_auto_fixes(
            &self.client,
            self.city.center_lat,
            self.city.center_lng,
        )
        .await?;

        // Phase 2: Heuristic triage (cheap graph queries, no LLM)
        let suspects = triage::triage_suspects(&self.client, &from, &to).await?;
        stats.signals_checked = suspects.iter()
            .filter(|s| s.label != "Story")
            .count() as u64;
        stats.stories_checked = suspects.iter()
            .filter(|s| s.label == "Story")
            .count() as u64;

        // Phase 3: LLM flag checks on suspects (budget-capped)
        let budget = BudgetTracker::new(self.max_llm_checks);
        let mut llm_issues = llm::check_suspects(
            suspects,
            &self.anthropic_api_key,
            &self.city.slug,
            &budget,
        )
        .await;

        // Set city on all issues (LLM module doesn't have city context)
        for issue in &mut llm_issues {
            issue.city = self.city.slug.clone();
        }

        // Phase 4: Persist issues and send notifications
        for issue in &llm_issues {
            match self.issues.create_if_new(issue).await {
                Ok(true) => {
                    stats.issues_created += 1;
                    if let Err(e) = self.notifier.send(issue).await {
                        warn!(error = %e, issue_type = %issue.issue_type, "Failed to send notification");
                    }
                }
                Ok(false) => {
                    // Duplicate — already open, skip notification
                }
                Err(e) => {
                    warn!(error = %e, "Failed to persist ValidationIssue");
                }
            }
        }

        info!(
            llm_budget_used = budget.used(),
            llm_budget_remaining = budget.remaining(),
            "LLM budget summary"
        );

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
