pub mod config;
pub mod deps;
pub mod ingestor;
pub mod types;

pub use config::AppConfig;
pub use deps::{EmbeddingService, ServerDeps};
pub use ingestor::{DiscoverConfig, Ingestor, WebSearcher};
pub use types::*;
