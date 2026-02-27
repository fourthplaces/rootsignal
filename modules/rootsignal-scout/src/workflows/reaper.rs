//! Restate virtual object for reaping expired signals on a durable schedule.
//!
//! Runs as a singleton (key `"global"`). After each run it self-reschedules
//! via `send_after`, giving us crash-safe periodic reaping with full
//! visibility in the Restate admin dashboard.

use std::sync::Arc;
use std::time::Duration;

use restate_sdk::prelude::*;
use tracing::info;

use super::ScoutDeps;
use crate::impl_restate_serde;
use crate::traits::SignalReader;

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
        let deps = self.deps.clone();

        let result = ctx
            .run(|| async move {
                let run_id = format!("reaper-{}", uuid::Uuid::new_v4());
                let store = deps.build_store();
                let engine = deps.build_engine(&run_id);

                let expired = store
                    .find_expired_signals()
                    .await
                    .map_err(|e| -> HandlerError { e.into() })?;

                let mut gatherings = 0u64;
                let mut needs = 0u64;
                let mut stale = 0u64;

                let dummy_store: Arc<dyn SignalReader> =
                    Arc::new(crate::store::build_signal_reader(deps.graph_client.clone()));
                let pipe_deps = crate::pipeline::state::PipelineDeps {
                    store: dummy_store,
                    embedder: Arc::new(crate::infra::embedder::NoOpEmbedder)
                        as Arc<dyn crate::infra::embedder::TextEmbedder>,
                    region: None,
                    run_id,
                    fetcher: None,
                    anthropic_api_key: None,
                };
                let mut state =
                    crate::pipeline::state::PipelineState::new(std::collections::HashMap::new());

                for (signal_id, node_type, reason) in &expired {
                    let event = crate::pipeline::events::ScoutEvent::System(
                        rootsignal_common::events::SystemEvent::EntityExpired {
                            signal_id: *signal_id,
                            node_type: *node_type,
                            reason: reason.clone(),
                        },
                    );
                    if let Err(e) = engine.dispatch(event, &mut state, &pipe_deps).await {
                        tracing::warn!(error = %e, signal_id = %signal_id, "Failed to expire signal");
                        continue;
                    }
                    match node_type {
                        rootsignal_common::types::NodeType::Gathering => gatherings += 1,
                        rootsignal_common::types::NodeType::Need => needs += 1,
                        _ => stale += 1,
                    }
                }

                Ok(ReapResult {
                    gatherings,
                    needs,
                    stale,
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
