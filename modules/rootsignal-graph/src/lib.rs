pub mod cache;
pub mod cached_reader;
pub mod cause_heat;
pub mod client;
pub mod migrate;
pub mod reader;
pub mod response;
pub mod similarity;
pub mod story_metrics;
pub mod story_weaver;
pub mod synthesizer;
#[cfg(feature = "test-utils")]
pub mod testutil;
pub mod writer;

pub use cache::CacheStore;
pub use cached_reader::CachedReader;
pub use client::GraphClient;
pub use reader::{PublicGraphReader, ResourceGap, ResourceMatch};
pub use similarity::SimilarityBuilder;
pub use story_metrics::{parse_recency, story_energy, story_status};
pub use story_weaver::StoryWeaver;
pub use synthesizer::Synthesizer;
pub use writer::{
    ConsolidationStats, DuplicateMatch, EvidenceSummary, ExtractionYield, GapTypeStats,
    GatheringFinderTarget, GraphWriter, InvestigationTarget, ReapStats, ResponseFinderTarget,
    ResponseHeuristic, SignalTypeCounts, SourceBrief, SourceStats, StoryBrief, StoryGrowth,
    TensionHub, TensionLinkerOutcome, TensionLinkerTarget, TensionRespondent, TensionResponseShape,
    UnmetTension,
};

/// Re-export neo4rs::query for downstream crates that need raw Cypher access (e.g. test assertions).
pub use neo4rs::query;
