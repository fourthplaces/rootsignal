pub mod firecrawl;
pub mod http;
pub mod tavily;

use anyhow::Result;
use std::sync::Arc;
use taproot_core::{Ingestor, WebSearcher};

/// Build an ingestor based on adapter name from source config.
pub fn build_ingestor(
    adapter: &str,
    http_client: &reqwest::Client,
    firecrawl_api_key: Option<&str>,
) -> Result<Arc<dyn Ingestor>> {
    match adapter {
        "firecrawl" => {
            let key = firecrawl_api_key
                .ok_or_else(|| anyhow::anyhow!("FIRECRAWL_API_KEY required for firecrawl adapter"))?;
            Ok(Arc::new(firecrawl::FirecrawlIngestor::new(
                key.to_string(),
                http_client.clone(),
            )))
        }
        "http" => Ok(Arc::new(http::HttpIngestor::new(http_client.clone()))),
        "tavily" => Err(anyhow::anyhow!(
            "Tavily is a search adapter, not an ingestor. Use build_web_searcher instead."
        )),
        other => Err(anyhow::anyhow!("Unknown adapter: {}", other)),
    }
}

/// Build the web searcher (Tavily).
pub fn build_web_searcher(
    tavily_api_key: &str,
    http_client: &reqwest::Client,
) -> Arc<dyn WebSearcher> {
    Arc::new(tavily::TavilySearcher::new(
        tavily_api_key.to_string(),
        http_client.clone(),
    ))
}
