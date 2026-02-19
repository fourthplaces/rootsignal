pub mod cause_heat;
pub mod client;
pub mod cluster;
pub mod reader;
pub mod response;
pub mod similarity;
pub mod story_weaver;
pub mod synthesizer;
#[cfg(feature = "test-utils")]
pub mod testutil;
pub mod writer;
pub mod migrate;

pub use client::GraphClient;
pub use cluster::Clusterer;
pub use reader::{PublicGraphReader, ResourceGap, ResourceMatch};
pub use similarity::SimilarityBuilder;
pub use story_weaver::StoryWeaver;
pub use synthesizer::Synthesizer;
pub use writer::{
    ConsolidationStats, CuriosityOutcome, CuriosityTarget, DuplicateMatch, EvidenceSummary,
    ExtractionYield, GapTypeStats, GravityScoutTarget, GraphWriter, InvestigationTarget,
    ReapStats, ResponseHeuristic, ResponseScoutTarget, SignalTypeCounts, SourceBrief,
    SourceStats, StoryBrief, StoryGrowth, TensionHub, TensionRespondent, TensionResponseShape,
    UnmetTension,
};

/// Re-export neo4rs::query for downstream crates that need raw Cypher access (e.g. test assertions).
pub use neo4rs::query;
