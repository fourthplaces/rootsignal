// New structure (Phase 1+)
pub mod core;
pub mod domains;

// Existing modules (coexist until migration complete)
pub mod discovery;
pub mod enrichment;
pub mod infra;
pub mod news_scanner;
pub mod scheduling;
pub mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod traits;
pub mod workflows;
