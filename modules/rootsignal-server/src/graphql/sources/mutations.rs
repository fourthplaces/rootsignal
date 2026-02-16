use async_graphql::*;
use std::sync::Arc;
use uuid::Uuid;

use super::types::GqlSource;
use crate::graphql::auth::middleware::require_admin;
use crate::graphql::entities::types::GqlEntity;
use crate::graphql::error;
use crate::graphql::workflows::trigger_restate_workflow;
use rootsignal_core::ServerDeps;

#[derive(Default)]
pub struct SourceMutation;

#[Object]
impl SourceMutation {
    /// Create a source from a URL or search query.
    /// The backend auto-detects source type, name, and handle from the input.
    /// Returns the existing source if the normalized URL already exists.
    async fn add_source(&self, ctx: &Context<'_>, input: String) -> Result<GqlSource> {
        tracing::info!(input = %input, "graphql.add_source");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let source = rootsignal_domains::scraping::Source::create_from_input(&input, pool)
            .await
            .map_err(|e| error::internal(e))?;

        tracing::info!(id = %source.id, name = %source.name, "graphql.add_source.ok");
        Ok(GqlSource::from(source))
    }

    async fn activate_sources(&self, ctx: &Context<'_>, ids: Vec<Uuid>) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.activate_sources");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let updated = rootsignal_domains::scraping::Source::set_active_many(&ids, true, pool)
            .await
            .map_err(|e| error::internal(e))?;

        tracing::info!(updated = updated, "graphql.activate_sources.ok");
        Ok(updated as i32)
    }

    async fn deactivate_sources(&self, ctx: &Context<'_>, ids: Vec<Uuid>) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.deactivate_sources");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let updated = rootsignal_domains::scraping::Source::set_active_many(&ids, false, pool)
            .await
            .map_err(|e| error::internal(e))?;

        tracing::info!(updated = updated, "graphql.deactivate_sources.ok");
        Ok(updated as i32)
    }

    async fn scrape_sources(&self, ctx: &Context<'_>, ids: Vec<Uuid>) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.scrape_sources");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?.clone();

        let mut triggered = 0;
        for id in &ids {
            let key = format!("{}-{}", id, chrono::Utc::now().timestamp());
            if let Err(e) = trigger_restate_workflow(
                &deps,
                "ScrapeWorkflow",
                &key,
                serde_json::json!({ "source_id": id.to_string() }),
            )
            .await
            {
                tracing::warn!(source_id = %id, error = ?e, "Failed to trigger scrape");
            } else {
                triggered += 1;
            }
        }

        tracing::info!(triggered = triggered, "graphql.scrape_sources.ok");
        Ok(triggered)
    }

    async fn delete_sources(&self, ctx: &Context<'_>, ids: Vec<Uuid>) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.delete_sources");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let deleted = rootsignal_domains::scraping::Source::delete_many(&ids, pool)
            .await
            .map_err(|e| error::internal(e))?;

        tracing::info!(deleted = deleted, "graphql.delete_sources.ok");
        Ok(deleted as i32)
    }

    /// Use AI to detect the entity behind a source from its scraped pages,
    /// then find-or-create the entity and link it to the source.
    async fn detect_source_entity(&self, ctx: &Context<'_>, source_id: Uuid) -> Result<GqlEntity> {
        tracing::info!(source_id = %source_id, "graphql.detect_source_entity");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let entity =
            rootsignal_domains::scraping::activities::detect_source_entity(source_id, deps)
                .await
                .map_err(|e| error::internal(e))?;

        Ok(GqlEntity::from(entity))
    }
}
