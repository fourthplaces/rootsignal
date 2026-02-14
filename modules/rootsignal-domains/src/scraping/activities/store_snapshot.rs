use anyhow::Result;
use sqlx::PgPool;
use rootsignal_core::RawPage;
use uuid::Uuid;

/// Store a raw page as an immutable page_snapshot. Returns the snapshot ID.
/// Deduplicates by (url, content_hash).
pub async fn store_page_snapshot(
    page: &RawPage,
    source_id: Uuid,
    pool: &PgPool,
) -> Result<Uuid> {
    let content_hash = page.content_hash();
    let metadata = serde_json::to_value(&page.metadata)?;

    // Upsert page_snapshot (immutable — conflict means we already have it)
    let snapshot = sqlx::query_as::<_, (Uuid,)>(
        r#"
        INSERT INTO page_snapshots (url, content_hash, html, markdown, fetched_via, metadata, crawled_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (url, content_hash) DO UPDATE SET url = EXCLUDED.url
        RETURNING id
        "#,
    )
    .bind(&page.url)
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
