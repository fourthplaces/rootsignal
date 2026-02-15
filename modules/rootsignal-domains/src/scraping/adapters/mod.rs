pub mod facebook;
pub mod firecrawl;
pub mod gofundme;
pub mod http;
pub mod instagram;
pub mod tavily;
pub mod tiktok;
pub mod x;

use anyhow::Result;
use rootsignal_core::{Ingestor, ValidatedIngestor, WebSearcher};
use std::sync::Arc;

/// Build an ingestor based on adapter name from source config.
///
/// All URL-based ingestors are wrapped with `ValidatedIngestor` for SSRF protection.
/// Apify-based ingestors hit api.apify.com only, so they skip SSRF wrapping.
pub fn build_ingestor(
    adapter: &str,
    http_client: &reqwest::Client,
    firecrawl_api_key: Option<&str>,
    apify_api_key: Option<&str>,
) -> Result<Arc<dyn Ingestor>> {
    match adapter {
        "firecrawl" => {
            let key = firecrawl_api_key.ok_or_else(|| {
                anyhow::anyhow!("FIRECRAWL_API_KEY required for firecrawl adapter")
            })?;
            let ingestor = firecrawl::FirecrawlIngestor::new(key.to_string(), http_client.clone());
            Ok(Arc::new(ValidatedIngestor::new(ingestor)))
        }
        "http" => {
            let ingestor = http::HttpIngestor::new(http_client.clone());
            Ok(Arc::new(ValidatedIngestor::new(ingestor)))
        }
        "apify_instagram" => {
            let key = apify_api_key.ok_or_else(|| {
                anyhow::anyhow!("APIFY_API_KEY required for apify_instagram adapter")
            })?;
            Ok(Arc::new(instagram::InstagramIngestor::new(key.to_string())))
        }
        "apify_facebook" => {
            let key = apify_api_key.ok_or_else(|| {
                anyhow::anyhow!("APIFY_API_KEY required for apify_facebook adapter")
            })?;
            Ok(Arc::new(facebook::FacebookIngestor::new(key.to_string())))
        }
        "apify_x" => {
            let key = apify_api_key
                .ok_or_else(|| anyhow::anyhow!("APIFY_API_KEY required for apify_x adapter"))?;
            Ok(Arc::new(x::XIngestor::new(key.to_string())))
        }
        "apify_tiktok" => {
            let key = apify_api_key.ok_or_else(|| {
                anyhow::anyhow!("APIFY_API_KEY required for apify_tiktok adapter")
            })?;
            Ok(Arc::new(tiktok::TikTokIngestor::new(key.to_string())))
        }
        "apify_gofundme" => {
            let key = apify_api_key.ok_or_else(|| {
                anyhow::anyhow!("APIFY_API_KEY required for apify_gofundme adapter")
            })?;
            Ok(Arc::new(gofundme::GoFundMeIngestor::new(key.to_string())))
        }
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
