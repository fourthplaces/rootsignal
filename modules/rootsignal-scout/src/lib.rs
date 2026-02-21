pub mod archive_bridge;
pub mod discovery;
pub mod enrichment;
pub mod infra;
pub mod news_scanner;
pub mod pipeline;
pub mod run_log;
pub mod scheduling;

pub mod fixtures;
pub mod scout;

// Re-export submodules at crate root for backwards compatibility
pub use discovery::{gathering_finder, response_finder, source_finder, tension_linker};
pub use enrichment::{actor_extractor, expansion, investigator, quality};
pub use infra::{embedder, util};
pub use pipeline::{extractor, scrape_phase, scraper, sources};
pub use scheduling::{bootstrap, budget, metrics, scheduler};
