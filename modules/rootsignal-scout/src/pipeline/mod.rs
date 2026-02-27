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
