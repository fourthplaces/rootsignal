//! Pipeline — orchestrates reducer + enrichment into a complete graph build.
//!
//! The pipeline sequences two steps:
//! 1. **Reduce**: apply factual events to the graph (via GraphReducer)
//! 2. **Enrich**: compute derived properties from graph state (diversity, actor stats, cause_heat)
//!
//! Replay guarantee: the same events always produce the same graph. Enrichment is
//! deterministically recomputed from the graph state that the reducer produced.

use anyhow::Result;
use tracing::info;

use rootsignal_common::EntityMappingOwned;
use rootsignal_events::{EventStore, StoredEvent};

use crate::enrich::{enrich, EnrichStats};
use crate::reducer::{ApplyResult, GraphReducer};
use crate::GraphClient;

/// Stats from a full pipeline run.
#[derive(Debug)]
pub struct PipelineStats {
    pub events_applied: u32,
    pub events_noop: u32,
    pub events_error: u32,
    pub enrich: EnrichStats,
}

/// Bbox for cause_heat computation.
#[derive(Debug, Clone)]
pub struct BBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
}

/// Orchestrates reducer + enrichment into a complete graph build.
pub struct Pipeline {
    client: GraphClient,
    reducer: GraphReducer,
    cause_heat_threshold: f64,
}

impl Pipeline {
    pub fn new(client: GraphClient, cause_heat_threshold: f64) -> Self {
        let reducer = GraphReducer::new(client.clone());
        Self {
            client,
            reducer,
            cause_heat_threshold,
        }
    }

    /// Process a batch of events through the full pipeline: reduce → enrich.
    pub async fn process(
        &self,
        events: &[StoredEvent],
        bbox: &BBox,
        entity_mappings: &[EntityMappingOwned],
    ) -> Result<PipelineStats> {
        let (mut applied, mut noop, mut errors) = (0u32, 0u32, 0u32);

        for event in events {
            match self.reducer.apply(event).await? {
                ApplyResult::Applied => applied += 1,
                ApplyResult::NoOp => noop += 1,
                ApplyResult::DeserializeError(_) => errors += 1,
            }
        }

        let enrich_stats = enrich(
            &self.client,
            entity_mappings,
            self.cause_heat_threshold,
            bbox.min_lat,
            bbox.max_lat,
            bbox.min_lng,
            bbox.max_lng,
        )
        .await?;

        Ok(PipelineStats {
            events_applied: applied,
            events_noop: noop,
            events_error: errors,
            enrich: enrich_stats,
        })
    }

    /// Full rebuild: wipe graph, replay all events, enrich.
    pub async fn rebuild(
        &self,
        store: &EventStore,
        bbox: &BBox,
        entity_mappings: &[EntityMappingOwned],
    ) -> Result<PipelineStats> {
        info!("Pipeline: full rebuild starting");

        let last_seq = self.reducer.rebuild(store).await?;

        let enrich_stats = enrich(
            &self.client,
            entity_mappings,
            self.cause_heat_threshold,
            bbox.min_lat,
            bbox.max_lat,
            bbox.min_lng,
            bbox.max_lng,
        )
        .await?;

        // Count events by reading from store (rebuild doesn't return per-event stats)
        let events = store.read_from(1, 1).await?;
        let total = if events.is_empty() { 0 } else { last_seq as u32 };

        info!(last_seq, "Pipeline: full rebuild complete");

        Ok(PipelineStats {
            events_applied: total,
            events_noop: 0, // rebuild doesn't track noop vs applied
            events_error: 0,
            enrich: enrich_stats,
        })
    }

    /// Replay from a specific sequence number, then enrich.
    pub async fn replay_from(
        &self,
        store: &EventStore,
        seq: i64,
        bbox: &BBox,
        entity_mappings: &[EntityMappingOwned],
    ) -> Result<PipelineStats> {
        info!(seq, "Pipeline: incremental replay starting");

        let last_seq = self.reducer.replay_from(store, seq).await?;

        let enrich_stats = enrich(
            &self.client,
            entity_mappings,
            self.cause_heat_threshold,
            bbox.min_lat,
            bbox.max_lat,
            bbox.min_lng,
            bbox.max_lng,
        )
        .await?;

        let applied = if last_seq >= seq {
            (last_seq - seq + 1) as u32
        } else {
            0
        };

        info!(last_seq, "Pipeline: incremental replay complete");

        Ok(PipelineStats {
            events_applied: applied,
            events_noop: 0,
            events_error: 0,
            enrich: enrich_stats,
        })
    }

    /// Access the underlying GraphClient.
    pub fn client(&self) -> &GraphClient {
        &self.client
    }

    /// Access the underlying GraphReducer.
    pub fn reducer(&self) -> &GraphReducer {
        &self.reducer
    }
}
