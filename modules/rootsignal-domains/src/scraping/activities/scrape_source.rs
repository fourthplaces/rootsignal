use anyhow::Result;
use rootsignal_core::{DiscoverConfig, ServerDeps};
use uuid::Uuid;

use crate::config::ServiceArea;
use crate::scraping::adapters;
use crate::scraping::Source;

/// Scrape a source, store page_snapshots, return snapshot IDs for extraction.
pub async fn scrape_source(source_id: Uuid, deps: &ServerDeps) -> Result<Vec<Uuid>> {
    let source = Source::find_by_id(source_id, deps.pool()).await?;
    tracing::info!(source_id = %source_id, name = %source.name, source_type = %source.source_type, "Scraping source");

    let pages = match source.source_type.as_str() {
        "web_search" => {
            // Web search source — use config.search_query or source name
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

            if query.contains("{location}") {
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
            }
        }
        source_type => {
            let adapter_name = match source_type {
                "instagram" => "apify_instagram",
                "facebook" => "apify_facebook",
                "x" => "apify_x",
                "tiktok" => "apify_tiktok",
                "gofundme" => "apify_gofundme",
                _ => "firecrawl",
            };
            let ingestor = adapters::build_ingestor(
                adapter_name,
                &deps.http_client,
                deps.config.firecrawl_api_key.as_deref(),
                deps.config.apify_api_key.as_deref(),
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

            // Pass handle from source config for social adapters
            if let Some(handle) = source.config.get("handle").and_then(|v| v.as_str()) {
                config = config.with_option("handle", handle);
            }

            // Apply include/exclude patterns from source config
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
                .map_err(|e| anyhow::anyhow!(e))?
        }
    };

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

    Ok(snapshot_ids)
}
