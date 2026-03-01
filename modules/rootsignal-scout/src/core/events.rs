//! Supporting types used across domain events.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelinePhase {
    ReapExpired,
    TensionScrape,
    MidRunDiscovery,
    ResponseScrape,
    Expansion,
    SocialScrape,
    SocialDiscovery,
    ActorEnrichment,
    Synthesis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessBucket {
    Within7d,
    Within30d,
    Within90d,
    Older,
    Unknown,
}
