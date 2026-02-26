//! Restate virtual object for reaping expired signals on a durable schedule.
//!
//! Runs as a singleton (key `"global"`). After each run it self-reschedules
//! via `send_after`, giving us crash-safe periodic reaping with full
//! visibility in the Restate admin dashboard.

use std::sync::Arc;
use std::time::Duration;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use super::ScoutDeps;
use crate::impl_restate_serde;

const REAP_INTERVAL: Duration = Duration::from_secs(6 * 3600); // 6 hours

/// Result returned from a reap run, visible in Restate invocation logs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReapResult {
    pub gatherings: u64,
    pub needs: u64,
    pub stale: u64,
}

impl_restate_serde!(ReapResult);

#[restate_sdk::object]
#[name = "SignalReaper"]
pub trait SignalReaper {
    async fn run() -> Result<ReapResult, HandlerError>;
}

pub struct SignalReaperImpl {
    deps: Arc<ScoutDeps>,
}

impl SignalReaperImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SignalReaper for SignalReaperImpl {
    async fn run(&self, ctx: ObjectContext<'_>) -> Result<ReapResult, HandlerError> {
        let gc = self.deps.graph_client.clone();

        let result = ctx
            .run(|| async move {
                let writer = GraphWriter::new(gc);
                let stats = writer
                    .reap_expired()
                    .await
                    .map_err(|e| -> HandlerError { e.into() })?;
                Ok(ReapResult {
                    gatherings: stats.gatherings,
                    needs: stats.needs,
                    stale: stats.stale,
                })
            })
            .await?;

        if result.gatherings + result.needs + result.stale > 0 {
            info!(
                gatherings = result.gatherings,
                needs = result.needs,
                stale = result.stale,
                "Expired signals removed"
            );
        } else {
            info!("No expired signals to reap");
        }

        // Self-reschedule for the next run
        ctx.object_client::<SignalReaperClient>(ctx.key())
            .run()
            .send_after(REAP_INTERVAL);

        info!("Next reap scheduled in {} hours", REAP_INTERVAL.as_secs() / 3600);

        Ok(result)
    }
}
