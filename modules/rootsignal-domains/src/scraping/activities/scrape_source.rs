use anyhow::Result;
use rootsignal_core::{DiscoverConfig, RawPage, ServerDeps};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::ServiceArea;
use crate::scraping::adapters;
use crate::scraping::Source;

/// Result of scraping a source.
///
/// For content sources (website, social): `snapshot_ids` has the stored page_snapshots.
/// For web_search sources: `discovered_pages` has the raw search results (no snapshots stored).
pub struct ScrapeOutput {
    pub snapshot_ids: Vec<Uuid>,
    pub source_type: String,
    pub discovered_pages: Vec<DiscoveredPage>,
}

/// A URL + title pair discovered via web search (not stored as a snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPage {
    pub url: String,
    pub title: String,
}

/// Scrape a source. For websites/social, fetches pages and stores snapshots.
/// For web_search, returns discovered URLs without storing anything.
pub async fn scrape_source(source_id: Uuid, deps: &ServerDeps) -> Result<ScrapeOutput> {
    let source = Source::find_by_id(source_id, deps.pool()).await?;
    tracing::info!(source_id = %source_id, name = %source.name, source_type = %source.source_type, "Scraping source");

    if source.source_type == "web_search" {
        return scrape_web_search(&source, source_id, deps).await;
    }

    let pages = scrape_content_source(&source, deps).await?;

    tracing::info!(source_id = %source_id, pages = pages.len(), "Fetched pages");

    let mut snapshot_ids = Vec::new();
    for page in &pages {
        match super::store_page_snapshot(page, source_id, deps.pool()).await {
            Ok(id) => snapshot_ids.push(id),
            Err(e) => tracing::warn!(url = %page.url, error = %e, "Failed to store snapshot"),
        }
    }

    Source::update_last_scraped(source_id, deps.pool()).await?;
    tracing::info!(source_id = %source_id, snapshots = snapshot_ids.len(), "Stored snapshots");

    Ok(ScrapeOutput {
        snapshot_ids,
        source_type: source.source_type,
        discovered_pages: vec![],
    })
}

/// Web search: run the query, return discovered URLs. No snapshots stored.
async fn scrape_web_search(
    source: &Source,
    source_id: Uuid,
    deps: &ServerDeps,
) -> Result<ScrapeOutput> {
    let query = source
        .config
        .get("search_query")
        .and_then(|v| v.as_str())
        .unwrap_or(&source.name);
    let max_results = source
        .config
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as u32;

    let pages = if query.contains("{location}") {
        let areas = ServiceArea::find_active(deps.pool()).await?;
        let mut all_pages = Vec::new();
        for area in &areas {
            let expanded = query.replace("{location}", &area.location_label());
            tracing::info!(source_id = %source_id, query = %expanded, "Expanded location query");
            let mut pages = deps.web_searcher.search(&expanded, max_results).await?;
            all_pages.append(&mut pages);
        }
        all_pages
    } else {
        deps.web_searcher.search(query, max_results).await?
    };

    let discovered_pages: Vec<DiscoveredPage> = pages
        .into_iter()
        .map(|p| DiscoveredPage {
            title: p.title.unwrap_or_else(|| p.url.clone()),
            url: p.url,
        })
        .collect();

    tracing::info!(source_id = %source_id, discovered = discovered_pages.len(), "Web search complete");
    Source::update_last_scraped(source_id, deps.pool()).await?;

    Ok(ScrapeOutput {
        snapshot_ids: vec![],
        source_type: source.source_type.clone(),
        discovered_pages,
    })
}

/// Content sources (website, social, etc.): fetch pages via the appropriate adapter.
async fn scrape_content_source(source: &Source, deps: &ServerDeps) -> Result<Vec<RawPage>> {
    let adapter_name = match source.source_type.as_str() {
        "instagram" => "apify_instagram",
        "facebook" => "apify_facebook",
        "x" => "apify_x",
        "tiktok" => "apify_tiktok",
        "gofundme" => "apify_gofundme",
        _ => "spider",
    };
    let ingestor = adapters::build_ingestor(
        adapter_name,
        &deps.http_client,
        deps.config.firecrawl_api_key.as_deref(),
        deps.config.apify_api_key.as_deref(),
        deps.config.chrome_url.as_deref(),
    )?;

    let url = source.url.as_deref().unwrap_or_default();
    let mut config = DiscoverConfig::new(url)
        .with_max_depth(
            source
                .config
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(2) as usize,
        )
        .with_limit(
            source
                .config
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize,
        );

    if let Some(handle) = source.config.get("handle").and_then(|v| v.as_str()) {
        config = config.with_option("handle", handle);
    }

    if let Some(includes) = source
        .config
        .get("include_patterns")
        .and_then(|v| v.as_array())
    {
        for pattern in includes {
            if let Some(p) = pattern.as_str() {
                config = config.include(p);
            }
        }
    }
    if let Some(excludes) = source
        .config
        .get("exclude_patterns")
        .and_then(|v| v.as_array())
    {
        for pattern in excludes {
            if let Some(p) = pattern.as_str() {
                config = config.exclude(p);
            }
        }
    }

    ingestor
        .discover(&config)
        .await
        .map_err(|e| anyhow::anyhow!(e))
}
