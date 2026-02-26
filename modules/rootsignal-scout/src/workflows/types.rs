//! Shared request/response types for scout workflows.
//!
//! All types implement `serde::{Serialize, Deserialize}` plus the Restate SDK
//! serialization traits via `impl_restate_serde!`.

use std::collections::{HashMap, HashSet};
use std::fmt;

use chrono::{DateTime, Utc};
use rootsignal_common::{ActorContext, ScoutScope, SourceNode};
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
    Scraping,
    Synthesis,
    SituationWeaving,
    SignalLint,
    Supervisor,
    Complete,
}

impl fmt::Display for WorkflowPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Bootstrap => write!(f, "Running bootstrap..."),
            Self::Scraping => write!(f, "Scraping sources..."),
            Self::Synthesis => write!(f, "Running synthesis..."),
            Self::SituationWeaving => write!(f, "Weaving situations..."),
            Self::SignalLint => write!(f, "Linting signals..."),
            Self::Supervisor => write!(f, "Running supervisor..."),
            Self::Complete => write!(f, "Full scout run complete"),
        }
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Input for workflows that operate on a specific task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub task_id: String,
    pub scope: ScoutScope,
}

/// Input for workflows that receive a running budget total.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetedTaskRequest {
    pub task_id: String,
    pub scope: ScoutScope,
    /// Cumulative cents spent by prior workflows in the pipeline.
    pub spent_cents: u64,
}

/// Empty request for `get_status` shared handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest;

/// Input for the single-URL scrape workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeUrlRequest {
    pub url: String,
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapResult {
    pub sources_created: u32,
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
pub struct SignalLintResult {
    pub passed: u32,
    pub corrected: u32,
    pub rejected: u32,
    pub situations_promoted: u32,
    pub stories_promoted: u32,
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
pub struct ScrapeUrlResult {
    pub signals_stored: u32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullRunResult {
    pub sources_created: u32,
    pub urls_scraped: u32,
    pub signals_stored: u32,
    pub issues_found: u32,
}

// ---------------------------------------------------------------------------
// Scrape workflow journaled types
// ---------------------------------------------------------------------------

/// Source scheduling data (fully serializable, no capabilities).
/// Produced by the load-and-schedule step, consumed by scrape phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleData {
    pub all_sources: Vec<SourceNode>,
    pub scheduled_sources: Vec<SourceNode>,
    pub tension_phase_keys: HashSet<String>,
    pub response_phase_keys: HashSet<String>,
    pub scheduled_keys: HashSet<String>,
    pub consumed_pin_ids: Vec<uuid::Uuid>,
    pub actor_contexts: HashMap<String, ActorContext>,
    pub url_to_canonical_key: HashMap<String, String>,
    pub url_to_pub_date: HashMap<String, DateTime<Utc>>,
    pub run_id: String,
}

/// Outcome of scraping a single web URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlOutcome {
    pub url: String,
    pub canonical_key: String,
    pub signals_stored: u32,
    /// True when the extractor produced at least one node (before dedup/filtering).
    pub had_extracted_nodes: bool,
    pub status: UrlStatus,
    pub expansion_queries: Vec<String>,
    pub collected_links: Vec<SerializableLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UrlStatus {
    Scraped,
    Unchanged,
    Failed,
}

/// Outcome of scraping a single social account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialOutcome {
    pub canonical_key: String,
    pub signals_stored: u32,
    pub post_count: u32,
    pub expansion_queries: Vec<String>,
    pub collected_links: Vec<SerializableLink>,
}

/// Resolved URL from search query or RSS feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedUrl {
    pub url: String,
    pub canonical_key: String,
    pub pub_date: Option<DateTime<Utc>>,
}

/// Serializable link discovered during scraping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableLink {
    pub url: String,
    pub discovered_on: String,
}

// ---------------------------------------------------------------------------
// Restate serde impls
// ---------------------------------------------------------------------------

crate::impl_restate_serde!(TaskRequest);
crate::impl_restate_serde!(BudgetedTaskRequest);
crate::impl_restate_serde!(EmptyRequest);
crate::impl_restate_serde!(ScrapeUrlRequest);
crate::impl_restate_serde!(ScrapeUrlResult);
crate::impl_restate_serde!(BootstrapResult);
crate::impl_restate_serde!(ScrapeResult);
crate::impl_restate_serde!(SynthesisResult);
crate::impl_restate_serde!(SituationWeaverResult);
crate::impl_restate_serde!(SignalLintResult);
crate::impl_restate_serde!(SupervisorResult);
crate::impl_restate_serde!(NewsScanResult);
crate::impl_restate_serde!(FullRunResult);
crate::impl_restate_serde!(ScheduleData);
crate::impl_restate_serde!(UrlOutcome);
crate::impl_restate_serde!(SocialOutcome);
crate::impl_restate_serde!(ResolvedUrl);
