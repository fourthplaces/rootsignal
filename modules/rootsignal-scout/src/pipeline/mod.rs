pub mod expansion;
pub mod extractor;
pub mod handlers;
pub mod scrape_phase;
pub mod scrape_pipeline;

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod chain_tests;
#[cfg(test)]
pub mod simweb_adapter;

/// Type alias for the scout engine — raw seesaw engine with ScoutEngineDeps.
pub type ScoutEngine = crate::core::engine::SeesawEngine;
