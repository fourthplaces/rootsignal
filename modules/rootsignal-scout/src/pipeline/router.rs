//! ScoutRouter — routes events to handlers.
//!
//! Pipeline events dispatch to handler functions.
//! World and System events are projected to Neo4j via the optional GraphProjector.

use anyhow::Result;
use async_trait::async_trait;
use rootsignal_engine::Router;
use rootsignal_events::StoredEvent;
use rootsignal_graph::GraphProjector;

use crate::pipeline::events::ScoutEvent;
use crate::pipeline::handlers;
use crate::pipeline::state::{PipelineDeps, PipelineState};

pub struct ScoutRouter {
    projector: Option<GraphProjector>,
}

impl ScoutRouter {
    pub fn new(projector: Option<GraphProjector>) -> Self {
        Self { projector }
    }
}

#[async_trait]
impl Router<ScoutEvent, PipelineState, PipelineDeps> for ScoutRouter {
    async fn route(
        &self,
        event: &ScoutEvent,
        stored: &StoredEvent,
        state: &mut PipelineState,
        deps: &PipelineDeps,
    ) -> Result<Vec<ScoutEvent>> {
        match event {
            ScoutEvent::Pipeline(pe) => handlers::route_pipeline(pe, stored, state, deps).await,
            ScoutEvent::World(_) | ScoutEvent::System(_) => {
                if let Some(proj) = &self.projector {
                    proj.project(stored).await?;
                }
                Ok(vec![])
            }
        }
    }
}
