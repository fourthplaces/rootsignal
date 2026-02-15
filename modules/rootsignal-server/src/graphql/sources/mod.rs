pub mod mutations;
pub mod types;

use std::sync::Arc;

use async_graphql::*;
use pgvector::Vector;
use rootsignal_core::ServerDeps;
use types::{GqlPageSnapshot, GqlPageSnapshotDetail, GqlSource};
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;

#[derive(Default)]
pub struct SourceQuery;

#[Object]
impl SourceQuery {
    async fn sources(&self, ctx: &Context<'_>) -> Result<Vec<GqlSource>> {
        tracing::info!("graphql.sources");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let sources = rootsignal_domains::scraping::Source::find_all(pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?;
        Ok(sources.into_iter().map(GqlSource::from).collect())
    }

    async fn source(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlSource> {
        tracing::info!(id = %id, "graphql.source");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let source = rootsignal_domains::scraping::Source::find_by_id(id, pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("source not found: {e}")))?;
        Ok(GqlSource::from(source))
    }

    async fn source_page_snapshots(
        &self,
        ctx: &Context<'_>,
        source_id: Uuid,
    ) -> Result<Vec<GqlPageSnapshot>> {
        tracing::info!(source_id = %source_id, "graphql.source_page_snapshots");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let rows = sqlx::query_as::<_, (Uuid, String, String, Vec<u8>, String, Option<String>, chrono::DateTime<chrono::Utc>, String)>(
            r#"
            SELECT ps.id, ds.page_url, ps.url, ps.content_hash, ps.fetched_via,
                   LEFT(ps.raw_content, 200) AS content_preview,
                   ps.crawled_at, ds.scrape_status
            FROM domain_snapshots ds
            JOIN page_snapshots ps ON ps.id = ds.page_snapshot_id
            WHERE ds.source_id = $1
            ORDER BY ps.crawled_at DESC
            "#,
        )
        .bind(source_id)
        .fetch_all(pool)
        .await
        .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|(id, page_url, url, content_hash, fetched_via, content_preview, crawled_at, scrape_status)| {
                GqlPageSnapshot {
                    id,
                    page_url,
                    url,
                    content_hash: content_hash.iter().map(|b| format!("{b:02x}")).collect(),
                    fetched_via,
                    content_preview,
                    crawled_at,
                    scrape_status,
                }
            })
            .collect())
    }

    async fn page_snapshot(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlPageSnapshotDetail> {
        tracing::info!(id = %id, "graphql.page_snapshot");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let row = sqlx::query_as::<_, (Uuid, String, String, Vec<u8>, String, Option<String>, Option<String>, serde_json::Value, chrono::DateTime<chrono::Utc>, String, Option<chrono::DateTime<chrono::Utc>>, Option<Uuid>)>(
            r#"
            SELECT ps.id, ps.url, ps.canonical_url, ps.content_hash, ps.fetched_via,
                   ps.html, ps.raw_content, COALESCE(ps.metadata, '{}'::jsonb) AS metadata,
                   ps.crawled_at, ps.extraction_status, ps.extraction_completed_at,
                   ds.source_id
            FROM page_snapshots ps
            LEFT JOIN domain_snapshots ds ON ds.page_snapshot_id = ps.id
            WHERE ps.id = $1
            "#,
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| async_graphql::Error::new(format!("snapshot not found: {e}")))?;

        let html = row.5;
        let raw_content = row.6;

        // Backward compat: if content is the same as html, compute plain text on the fly
        let content = match (&html, &raw_content) {
            (Some(h), Some(c)) if h == c => Some(rootsignal_core::html_to_plain_text(h)),
            _ => raw_content,
        };

        Ok(GqlPageSnapshotDetail {
            id: row.0,
            source_id: row.11,
            url: row.1,
            canonical_url: row.2,
            content_hash: row.3.iter().map(|b| format!("{b:02x}")).collect(),
            fetched_via: row.4,
            html,
            content,
            metadata: row.7,
            crawled_at: row.8,
            extraction_status: row.9,
            extraction_completed_at: row.10,
        })
    }

    async fn search_sources(&self, ctx: &Context<'_>, q: String) -> Result<Vec<GqlSource>> {
        tracing::info!(q = %q, "graphql.search_sources");
        require_admin(ctx)?;
        let deps = ctx.data_unchecked::<Arc<ServerDeps>>();
        let pool = deps.pool();

        let raw_embedding = deps
            .embedding_service
            .embed(&q)
            .await
            .map_err(|e| async_graphql::Error::new(format!("embedding error: {e}")))?;
        let query_vec = Vector::from(raw_embedding);

        let similar = rootsignal_domains::search::Embedding::search_similar(
            query_vec, "source", 50, 0.8, pool,
        )
        .await
        .map_err(|e| async_graphql::Error::new(format!("search error: {e}")))?;

        let ids: Vec<Uuid> = similar.iter().map(|s| s.embeddable_id).collect();

        // Fetch all sources and preserve similarity order
        let sources = rootsignal_domains::scraping::Source::find_all(pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?;

        let source_map: std::collections::HashMap<Uuid, _> =
            sources.into_iter().map(|s| (s.id, s)).collect();

        let ordered: Vec<GqlSource> = ids
            .into_iter()
            .filter_map(|id| source_map.get(&id).cloned().map(GqlSource::from))
            .collect();

        Ok(ordered)
    }
}
