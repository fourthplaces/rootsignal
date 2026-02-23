//! Shared request/response types for scout workflows.
//!
//! All types implement `serde::{Serialize, Deserialize}` plus the Restate SDK
//! serialization traits via `impl_restate_serde!`.

use std::fmt;

use rootsignal_common::ScoutScope;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Workflow phases
// ---------------------------------------------------------------------------

/// Named phases of a full scout run, used as status strings in Restate state.
///
/// String-compatible with the existing Restate state store — `to_string()`
/// produces the same values that were previously hard-coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowPhase {
    Pending,
    Bootstrap,
    ActorDiscovery,
    Scraping,
    Synthesis,
    SituationWeaving,
    Supervisor,
    Complete,
}

impl fmt::Display for WorkflowPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Bootstrap => write!(f, "Running bootstrap..."),
            Self::ActorDiscovery => write!(f, "Discovering actors..."),
            Self::Scraping => write!(f, "Scraping sources..."),
            Self::Synthesis => write!(f, "Running synthesis..."),
            Self::SituationWeaving => write!(f, "Weaving situations..."),
            Self::Supervisor => write!(f, "Running supervisor..."),
            Self::Complete => write!(f, "Full scout run complete"),
        }
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Input for workflows that operate on a region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionRequest {
    pub scope: ScoutScope,
}

/// Input for workflows that receive a running budget total.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetedRegionRequest {
    pub scope: ScoutScope,
    /// Cumulative cents spent by prior workflows in the pipeline.
    pub spent_cents: u64,
}

/// Empty request for `get_status` shared handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest;

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapResult {
    pub sources_created: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorDiscoveryResult {
    pub actors_discovered: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResult {
    pub urls_scraped: u32,
    pub signals_stored: u32,
    pub spent_cents: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisResult {
    pub spent_cents: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationWeaverResult {
    pub situations_woven: u32,
    pub spent_cents: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorResult {
    pub issues_found: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsScanResult {
    pub articles_scanned: u32,
    pub beacons_created: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullRunResult {
    pub sources_created: u32,
    pub actors_discovered: u32,
    pub urls_scraped: u32,
    pub signals_stored: u32,
    pub issues_found: u32,
}

// ---------------------------------------------------------------------------
// Actor service requests/responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFromPageRequest {
    pub url: String,
    pub fallback_region: String,
    pub require_social_links: bool,
    pub region_center_lat: f64,
    pub region_center_lng: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateManualActorRequest {
    pub name: String,
    pub actor_type: Option<String>,
    pub location: String,
    pub bio: Option<String>,
    pub social_accounts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAccountRequest {
    pub actor_id: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverActorsBatchRequest {
    pub query: String,
    pub region: String,
    pub max_results: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFromPageResult {
    pub actor_id: Option<String>,
    pub location_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateManualActorResult {
    pub actor_id: String,
    pub location_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAccountResult {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverActorsBatchResult {
    pub discovered: u32,
    pub actors: Vec<CreateFromPageResult>,
}

// ---------------------------------------------------------------------------
// Restate serde impls
// ---------------------------------------------------------------------------

crate::impl_restate_serde!(RegionRequest);
crate::impl_restate_serde!(BudgetedRegionRequest);
crate::impl_restate_serde!(EmptyRequest);
crate::impl_restate_serde!(BootstrapResult);
crate::impl_restate_serde!(ActorDiscoveryResult);
crate::impl_restate_serde!(ScrapeResult);
crate::impl_restate_serde!(SynthesisResult);
crate::impl_restate_serde!(SituationWeaverResult);
crate::impl_restate_serde!(SupervisorResult);
crate::impl_restate_serde!(NewsScanResult);
crate::impl_restate_serde!(FullRunResult);
crate::impl_restate_serde!(CreateFromPageRequest);
crate::impl_restate_serde!(CreateManualActorRequest);
crate::impl_restate_serde!(AddAccountRequest);
crate::impl_restate_serde!(DiscoverActorsBatchRequest);
crate::impl_restate_serde!(CreateFromPageResult);
crate::impl_restate_serde!(CreateManualActorResult);
crate::impl_restate_serde!(AddAccountResult);
crate::impl_restate_serde!(DiscoverActorsBatchResult);

// Newtype wrappers for ctx.run() journaling (orphan rule prevents impl on Vec<String> etc.)

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct UrlList(pub Vec<String>);
crate::impl_restate_serde!(UrlList);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MaybeActor(pub Option<CreateFromPageResult>);
crate::impl_restate_serde!(MaybeActor);
