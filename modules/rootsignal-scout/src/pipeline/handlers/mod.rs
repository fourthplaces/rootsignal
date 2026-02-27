//! Pipeline event handlers.
//!
//! Each handler receives a pipeline event, performs I/O via deps,
//! and returns child events that re-enter the dispatch loop.

pub(crate) mod bootstrap;
pub(crate) mod creation;
pub(crate) mod dedup;

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod dedup_tests;
#[cfg(test)]
mod engine_tests;

use anyhow::Result;
use rootsignal_events::StoredEvent;

use crate::pipeline::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::state::{PipelineDeps, PipelineState};

/// Dispatch a pipeline event to the appropriate handler.
pub async fn route_pipeline(
    event: &PipelineEvent,
    _stored: &StoredEvent,
    state: &PipelineState,
    deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    match event {
        // Extraction complete → run dedup on the stashed batch
        PipelineEvent::SignalsExtracted { url, .. } => {
            dedup::handle_signals_extracted(url, state, deps).await
        }

        // Dedup verdicts → creation / corroboration / freshness handlers
        PipelineEvent::NewSignalAccepted {
            node_id, source_url, ..
        } => creation::handle_create(*node_id, source_url, state, deps).await,
        PipelineEvent::CrossSourceMatchDetected {
            existing_id,
            node_type,
            source_url,
            similarity,
        } => {
            creation::handle_corroborate(*existing_id, *node_type, source_url, *similarity, deps)
                .await
        }
        PipelineEvent::SameSourceReencountered {
            existing_id,
            node_type,
            source_url,
            ..
        } => creation::handle_refresh(*existing_id, *node_type, source_url, deps).await,

        // Signal stored → wire edges (tags, source, actor)
        PipelineEvent::SignalReaderd {
            node_id,
            node_type,
            source_url,
            canonical_key,
        } => {
            creation::handle_signal_stored(
                *node_id,
                *node_type,
                source_url,
                canonical_key,
                state,
                deps,
            )
            .await
        }

        // Engine lifecycle — seed sources when region is empty
        PipelineEvent::EngineStarted { .. } => {
            bootstrap::handle_engine_started(state, deps).await
        }

        // Phase lifecycle and other informational events — no handler needed.
        _ => Ok(vec![]),
    }
}
