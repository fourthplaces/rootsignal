pub mod discovery;
pub mod enrichment;
pub mod infra;
// Used by rootsignal-api mutations
pub mod news_scanner;
pub mod pipeline;
pub mod run_log;
pub mod scheduling;
pub mod workflows;

// Used by integration tests (cannot be cfg(test) since tests are external)
pub mod fixtures;
pub mod scout;
