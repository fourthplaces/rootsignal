pub mod cause_heat;
pub mod client;
pub mod cluster;
pub mod edition;
pub mod reader;
pub mod response;
pub mod similarity;
pub mod synthesizer;
#[cfg(feature = "test-utils")]
pub mod testutil;
pub mod writer;
pub mod migrate;

pub use client::GraphClient;
pub use cluster::Clusterer;
pub use reader::PublicGraphReader;
pub use similarity::SimilarityBuilder;
pub use synthesizer::Synthesizer;
pub use writer::{DuplicateMatch, GraphWriter, InvestigationTarget, ReapStats, SourceStats};

/// Re-export neo4rs::query for downstream crates that need raw Cypher access (e.g. test assertions).
pub use neo4rs::query;
