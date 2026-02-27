pub mod events;
pub mod expansion;
pub mod extractor;
pub mod handlers;
pub mod reducer;
pub mod router;
pub mod scrape_phase;
pub mod scrape_pipeline;
pub mod state;
pub mod stats;

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod chain_tests;
#[cfg(test)]
pub mod simweb_adapter;

use std::sync::Arc;

use crate::pipeline::events::ScoutEvent;
use crate::pipeline::reducer::ScoutReducer;
use crate::pipeline::router::ScoutRouter;
use crate::pipeline::state::{PipelineDeps, PipelineState};

/// Type alias for the scout engine — persister is trait-erased so both
/// EventStore (production) and MemoryEventSink (tests) work.
pub type ScoutEngine = rootsignal_engine::Engine<
    ScoutEvent,
    PipelineState,
    PipelineDeps,
    ScoutReducer,
    ScoutRouter,
    Arc<dyn rootsignal_engine::EventPersister>,
>;
