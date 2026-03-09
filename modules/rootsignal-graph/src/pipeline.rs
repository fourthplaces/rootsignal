//! Pipeline — orchestrates projector + cause_heat into a complete graph build.
//!
//! The pipeline sequences two steps:
//! 1. **Project**: apply factual events to the graph (via GraphProjector).
//!    Embeddings are computed at projection time via EmbeddingStore.
//!    Diversity and actor stats are event-sourced (projected from events).
//! 2. **Cause heat**: compute cause_heat from graph state (depends on embeddings + diversity).
//!
//! Replay guarantee: the same events always produce the same graph.

use anyhow::Result;

use seesaw_core::types::PersistedEvent;

use crate::projector::{ApplyResult, GraphProjector};
use crate::GraphClient;

/// Stats from a full pipeline run.
#[derive(Debug)]
pub struct PipelineStats {
    pub events_applied: u32,
    pub events_noop: u32,
    pub events_error: u32,
    pub cause_heat_updated: u32,
}

/// Bbox for cause_heat computation.
#[derive(Debug, Clone)]
pub struct BBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
}

/// Orchestrates projector + cause_heat into a complete graph build.
pub struct Pipeline {
    client: GraphClient,
    projector: GraphProjector,
    cause_heat_threshold: f64,
}

impl Pipeline {
    pub fn new(client: GraphClient, cause_heat_threshold: f64) -> Self {
        let projector = GraphProjector::new(client.clone());
        Self {
            client,
            projector,
            cause_heat_threshold,
        }
    }

    /// Process a batch of events through the full pipeline: project → cause_heat.
    pub async fn process(
        &self,
        events: &[PersistedEvent],
        bbox: &BBox,
    ) -> Result<PipelineStats> {
        let (mut applied, mut noop, mut errors) = (0u32, 0u32, 0u32);

        for event in events {
            match self.projector.project(event).await? {
                ApplyResult::Applied => applied += 1,
                ApplyResult::NoOp => noop += 1,
                ApplyResult::DeserializeError(_) => errors += 1,
            }
        }

        let cause_heat_updated = crate::cause_heat::compute_cause_heat(
            &self.client,
            self.cause_heat_threshold,
            bbox.min_lat,
            bbox.max_lat,
            bbox.min_lng,
            bbox.max_lng,
        )
        .await
        .map(|_| 0u32)
        .unwrap_or(0);

        Ok(PipelineStats {
            events_applied: applied,
            events_noop: noop,
            events_error: errors,
            cause_heat_updated,
        })
    }

    /// Access the underlying GraphClient.
    pub fn client(&self) -> &GraphClient {
        &self.client
    }

    /// Access the underlying GraphProjector.
    pub fn projector(&self) -> &GraphProjector {
        &self.projector
    }
}
