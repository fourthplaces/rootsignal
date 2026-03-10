pub mod cache;
pub mod cached_reader;
pub mod cause_heat;
pub mod client;
pub mod embedding_store;
pub mod enrich;
pub mod geocoder;
pub mod migrate;
pub mod pipeline;
pub mod reader;
pub mod projector;
pub mod severity_inference;
pub mod similarity;
pub mod situation_temperature;
pub mod situation_weaver;
pub mod queries;
#[cfg(feature = "test-utils")]
pub mod testutil;
pub mod writer;

pub use cache::CacheStore;
pub use cached_reader::CachedReader;
pub use client::{connect_graph, GraphClient};
pub use embedding_store::EmbeddingStore;
pub use enrich::{compute_diversity_metrics, DiversityMetrics};
pub use pipeline::{BBox, Pipeline, PipelineStats};
pub use reader::{
    DiscoveryTreeRow, PublicGraphReader, ResourceGap, ResourceMatch, ValidationIssueRow,
    ValidationIssueSummary,
};
pub use projector::{ApplyResult, GraphProjector};
pub use writer::{
ConsolidationStats, DiscoveryTreeNode, DuplicateMatch, EvidenceSummary, ExtractionYield,
FieldCorrection, GapTypeStats, GatheringFinderTarget, GraphReader, GraphStore,
InvestigationTarget, NoticeInferenceRow, ReapStats, ResponseFinderTarget, ResponseHeuristic,
SignalBrief, SignalTypeCounts, SituationBrief, SourceBrief, SourceStats, StagedSignal, row_to_source_node,
ConcernHub, ConcernLinkerOutcome, ConcernLinkerTarget, ConcernRespondent, ConcernResponseShape,
UnmetTension, WeaveCandidate, WeaveSignal,
};
pub use queries::GraphQueries;
/// Re-export neo4rs::query for downstream crates that need raw Cypher access (e.g. test assertions).
pub use neo4rs::query;
