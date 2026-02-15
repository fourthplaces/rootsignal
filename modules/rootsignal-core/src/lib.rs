pub mod config;
pub mod deps;
pub mod error;
pub mod file_config;
pub mod ingestor;
pub mod memo;
pub mod prompt_registry;
pub mod security;
pub mod template;
pub mod types;

pub use config::AppConfig;
pub use deps::{EmbeddingService, ServerDeps};
pub use memo::MemoBuilder;
pub use error::{CrawlError, CrawlResult, SecurityError, SecurityResult};
pub use file_config::FileConfig;
pub use ingestor::{DiscoverConfig, Ingestor, ValidatedIngestor, WebSearcher};
pub use prompt_registry::PromptRegistry;
pub use security::UrlValidator;
pub use types::*;

pub fn html_to_plain_text(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 80).unwrap_or_default()
}
