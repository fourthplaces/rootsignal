use anyhow::Result;
use pgvector::Vector;
use rootsignal_core::{RawPage, ServerDeps};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::scraping::url_alias::{normalize_url, UrlAlias};
use crate::search::Embedding;

/// Store a raw page as an immutable page_snapshot. Returns the snapshot ID.
/// Deduplicates by (canonical_url, content_hash).
pub async fn store_page_snapshot(page: &RawPage, source_id: Uuid, deps: &ServerDeps) -> Result<Uuid> {
    let pool = deps.pool();
    let content_hash = page.content_hash();
    let metadata = serde_json::to_value(&page.metadata)?;

    // Normalize URL for deduplication
    let canonical_url = normalize_url(&page.url).unwrap_or_else(|_| page.url.clone());

    // If the canonical URL differs from the original, record the alias
    if canonical_url != page.url {
        let _ = UrlAlias::create(&page.url, &canonical_url, None, pool).await;
    }

    // Check if this URL is a known redirect alias
    if let Ok(Some(alias)) = UrlAlias::find_canonical(&canonical_url, pool).await {
        // Use the stored canonical URL if we already know about a redirect
        let _ = alias; // canonical_url is already normalized; alias lookup is for future redirect tracking
    }

    // Upsert page_snapshot (immutable — conflict means we already have it)
    let snapshot = sqlx::query_as::<_, (Uuid,)>(
        r#"
        INSERT INTO page_snapshots (url, canonical_url, content_hash, html, raw_content, fetched_via, metadata, crawled_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (canonical_url, content_hash) DO UPDATE SET url = EXCLUDED.url
        RETURNING id
        "#,
    )
    .bind(&page.url)
    .bind(&canonical_url)
    .bind(&content_hash)
    .bind(&page.html)
    .bind(&page.content)
    .bind(
        page.metadata
            .get("fetched_via")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
    )
    .bind(&metadata)
    .bind(page.fetched_at)
    .fetch_one(pool)
    .await?;

    let snapshot_id = snapshot.0;

    // Link source → page via domain_snapshots
    sqlx::query(
        r#"
        INSERT INTO domain_snapshots (source_id, page_url, page_snapshot_id, last_scraped_at, scrape_status)
        VALUES ($1, $2, $3, NOW(), 'completed')
        ON CONFLICT (source_id, page_url) DO UPDATE
        SET page_snapshot_id = $3, last_scraped_at = NOW(), scrape_status = 'completed'
        "#,
    )
    .bind(source_id)
    .bind(&page.url)
    .bind(snapshot_id)
    .execute(pool)
    .await?;

    // Embed the page content for semantic search
    if !page.content.is_empty() {
        {
            // Truncate to ~8K chars for embedding model token limit
            let embed_text = if page.content.len() > 8000 {
                &page.content[..8000]
            } else {
                page.content.as_str()
            };

            let mut hasher = Sha256::new();
            hasher.update(embed_text.as_bytes());
            let hash = hex::encode(hasher.finalize());

            match deps.embedding_service.embed(embed_text).await {
                Ok(raw_embedding) => {
                    let vector = Vector::from(raw_embedding);
                    match Embedding::upsert("page_snapshot", snapshot_id, "en", vector, &hash, pool).await {
                        Ok(_) => tracing::debug!(snapshot_id = %snapshot_id, chars = embed_text.len(), "Embedded page snapshot"),
                        Err(e) => tracing::warn!(snapshot_id = %snapshot_id, error = %e, "Failed to store page embedding"),
                    }
                }
                Err(e) => {
                    tracing::warn!(snapshot_id = %snapshot_id, error = %e, "Failed to embed page content");
                }
            }
        }
    }

    Ok(snapshot_id)
}
