//! Run scope: determines the modality of a scout run.

use rootsignal_common::{RegionNode, ScoutScope, SourceNode};
use serde::{Deserialize, Serialize};

/// What a scout run is scoped to.
///
/// - `Unscoped`: no geographic context (tests, news scans).
/// - `Region`: load sources from graph, full scheduling algorithm.
/// - `Sources`: scrape specific input sources, optional geographic context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RunScope {
    /// No geographic scope (tests, news scans).
    Unscoped,
    /// Region-wide: load sources from graph, full scheduling algorithm.
    Region(ScoutScope),
    /// Source-targeted: scrape these specific sources.
    Sources {
        sources: Vec<SourceNode>,
        region: Option<ScoutScope>,
    },
}

impl Default for RunScope {
    fn default() -> Self {
        Self::Unscoped
    }
}

impl RunScope {
    /// Geographic context, if available.
    ///
    /// Maps 1:1 to the old `deps.region.as_ref()`.
    pub fn region(&self) -> Option<&ScoutScope> {
        match self {
            Self::Unscoped => None,
            Self::Region(r) => Some(r),
            Self::Sources { region, .. } => region.as_ref(),
        }
    }

    /// Input sources for targeted runs.
    pub fn input_sources(&self) -> Option<&[SourceNode]> {
        match self {
            Self::Sources { sources, .. } => Some(sources),
            _ => None,
        }
    }
}
