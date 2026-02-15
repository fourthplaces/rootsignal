use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::scrape_source::DiscoveredPage;
use crate::scraping::Source;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub created: Vec<Uuid>,
    pub skipped: u32,
}

/// Create Source records from web search results.
/// Each URL becomes a website source (deduplicated by domain).
pub async fn discover_sources(
    pages: &[DiscoveredPage],
    parent_source_id: Uuid,
    pool: &PgPool,
) -> Result<DiscoveryResult> {
    let mut created = Vec::new();
    let mut skipped: u32 = 0;

    for page in pages {
        match Source::find_or_create_website(&page.title, &page.url, Some(parent_source_id), pool)
            .await
        {
            Ok((source, was_created)) => {
                if was_created {
                    tracing::info!(source_id = %source.id, url = %page.url, "Discovered new source");
                    created.push(source.id);
                } else {
                    skipped += 1;
                }
            }
            Err(e) => {
                tracing::warn!(url = %page.url, error = %e, "Failed to create source from URL");
                skipped += 1;
            }
        }
    }

    tracing::info!(
        parent = %parent_source_id,
        created = created.len(),
        skipped = skipped,
        "Source discovery complete"
    );

    Ok(DiscoveryResult { created, skipped })
}
