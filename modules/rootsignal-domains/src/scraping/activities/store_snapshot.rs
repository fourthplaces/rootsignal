use anyhow::Result;
use rootsignal_core::RawPage;
use sqlx::PgPool;
use uuid::Uuid;

use crate::scraping::url_alias::{normalize_url, UrlAlias};

/// Store a raw page as an immutable page_snapshot. Returns the snapshot ID.
/// Deduplicates by (canonical_url, content_hash).
pub async fn store_page_snapshot(page: &RawPage, source_id: Uuid, pool: &PgPool) -> Result<Uuid> {
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
        INSERT INTO page_snapshots (url, canonical_url, content_hash, html, markdown, fetched_via, metadata, crawled_at)
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

    Ok(snapshot_id)
}
