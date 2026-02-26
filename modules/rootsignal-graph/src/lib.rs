pub mod beacon;
pub mod cache;
pub mod cached_reader;
pub mod cause_heat;
pub mod client;
pub mod embedding_store;
pub mod enrich;
pub mod migrate;
pub mod pipeline;
pub mod reader;
pub mod reducer;
pub mod response;
pub mod severity_inference;
pub mod similarity;
pub mod situation_temperature;
pub mod situation_weaver;
pub mod embedding_enrichment;
#[cfg(feature = "test-utils")]
pub mod testutil;
pub mod writer;

pub use cache::CacheStore;
pub use cached_reader::CachedReader;
pub use client::GraphClient;
pub use reader::{PublicGraphReader, ResourceGap, ResourceMatch, ValidationIssueRow, ValidationIssueSummary};
pub use similarity::SimilarityBuilder;
pub use situation_weaver::SituationWeaver;
pub use embedding_store::EmbeddingStore;
pub use enrich::{enrich, EnrichStats};
pub use pipeline::{BBox, Pipeline, PipelineStats};
pub use embedding_enrichment::{enrich_embeddings, EmbeddingEnrichStats};
pub use reducer::{ApplyResult, GraphProjector};
pub use writer::{
    ConsolidationStats, DiscoveryTreeNode, DuplicateMatch, EvidenceSummary, ExtractionYield,
    FieldCorrection, GapTypeStats, GatheringFinderTarget, GraphWriter, InvestigationTarget,
    NoticeInferenceRow, ReapStats, ResponseFinderTarget, ResponseHeuristic, SignalBrief,
    SignalTypeCounts, SituationBrief, SourceBrief, SourceStats, StagedSignal,
    TensionHub, TensionLinkerOutcome, TensionLinkerTarget, TensionRespondent,
    TensionResponseShape, UnmetTension,
};

/// Re-export neo4rs::query for downstream crates that need raw Cypher access (e.g. test assertions).
pub use neo4rs::query;
