use anyhow::Result;
use tracing::{info, warn};

use rootsignal_common::CityNode;
use rootsignal_graph::GraphClient;

use crate::checks::auto_fix;
use crate::notify::backend::NotifyBackend;
use crate::state::SupervisorState;
use crate::types::SupervisorStats;

/// The scout supervisor: validates the graph and feeds back into scout behavior.
pub struct Supervisor {
    client: GraphClient,
    state: SupervisorState,
    city: CityNode,
    notifier: Box<dyn NotifyBackend>,
}

impl Supervisor {
    pub fn new(
        client: GraphClient,
        city: CityNode,
        notifier: Box<dyn NotifyBackend>,
    ) -> Self {
        let state = SupervisorState::new(client.clone(), city.slug.clone());
        Self {
            client,
            state,
            city,
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

        // Phase 1: Auto-fix checks (deterministic, safe to run anytime)
        stats.auto_fix = auto_fix::run_auto_fixes(
            &self.client,
            self.city.center_lat,
            self.city.center_lng,
        )
        .await?;

        // Phase 2: Send digest
        if let Err(e) = self.notifier.send_digest(&stats).await {
            warn!(error = %e, "Failed to send digest notification");
        }

        // Update watermark
        self.state.update_last_run(&to).await?;

        info!("Supervisor run complete. {stats}");
        Ok(stats)
    }
}
