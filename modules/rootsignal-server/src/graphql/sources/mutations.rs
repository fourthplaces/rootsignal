use async_graphql::*;
use std::sync::Arc;
use uuid::Uuid;

use super::types::GqlSource;
use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;
use crate::graphql::entities::types::GqlEntity;
use crate::graphql::workflows::trigger_restate_workflow;
use rootsignal_core::ServerDeps;

#[derive(InputObject)]
pub struct CreateSourceInput {
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub entity_id: Option<Uuid>,
    pub cadence_hours: Option<i32>,
    pub config: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct SourceMutation;

#[Object]
impl SourceMutation {
    async fn create_source(
        &self,
        ctx: &Context<'_>,
        input: CreateSourceInput,
    ) -> Result<GqlSource> {
        tracing::info!(name = %input.name, source_type = %input.source_type, "graphql.create_source");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let config = input
            .config
            .unwrap_or(serde_json::Value::Object(Default::default()));

        let source = rootsignal_domains::scraping::Source::create(
            &input.name,
            &input.source_type,
            input.url.as_deref(),
            input.handle.as_deref(),
            input.entity_id,
            input.cadence_hours,
            config,
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        tracing::info!(id = %source.id, "graphql.create_source.ok");

        // Auto-trigger qualification for new sources
        let deps = ctx.data::<Arc<ServerDeps>>()?.clone();
        let source_id = source.id;
        tokio::spawn(async move {
            if let Err(e) = trigger_restate_workflow(
                &deps,
                "QualifyWorkflow",
                &source_id.to_string(),
                serde_json::json!({}),
            ).await {
                tracing::warn!(source_id = %source_id, error = e.message, "Auto-qualification trigger failed");
            }
        });

        Ok(GqlSource::from(source))
    }

    async fn activate_sources(
        &self,
        ctx: &Context<'_>,
        ids: Vec<Uuid>,
    ) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.activate_sources");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let updated = rootsignal_domains::scraping::Source::set_active_many(&ids, true, pool)
            .await
            .map_err(|e| error::internal(e))?;

        tracing::info!(updated = updated, "graphql.activate_sources.ok");
        Ok(updated as i32)
    }

    async fn deactivate_sources(
        &self,
        ctx: &Context<'_>,
        ids: Vec<Uuid>,
    ) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.deactivate_sources");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let updated = rootsignal_domains::scraping::Source::set_active_many(&ids, false, pool)
            .await
            .map_err(|e| error::internal(e))?;

        tracing::info!(updated = updated, "graphql.deactivate_sources.ok");
        Ok(updated as i32)
    }

    async fn qualify_sources(
        &self,
        ctx: &Context<'_>,
        ids: Vec<Uuid>,
    ) -> Result<i32> {
        tracing::info!(count = ids.len(), "graphql.qualify_sources");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?.clone();

        let mut triggered = 0;
        for id in &ids {
            let key = format!("{}-{}", id, chrono::Utc::now().timestamp());
            if let Err(e) = trigger_restate_workflow(
                &deps,
                "QualifyWorkflow",
                &key,
                serde_json::json!({ "source_id": id.to_string() }),
            )
            .await
            {
                tracing::warn!(source_id = %id, error = ?e, "Failed to trigger qualification");
            } else {
                triggered += 1;
            }
        }

        tracing::info!(triggered = triggered, "graphql.qualify_sources.ok");
        Ok(triggered)
    }

    async fn delete_sources(
        &self,
        ctx: &Context<'_>,
        ids: Vec<Uuid>,
    ) -> Result<i32> {
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
    async fn detect_source_entity(
        &self,
        ctx: &Context<'_>,
        source_id: Uuid,
    ) -> Result<GqlEntity> {
        tracing::info!(source_id = %source_id, "graphql.detect_source_entity");
        require_admin(ctx)?;
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        let entity = rootsignal_domains::scraping::activities::detect_source_entity(
            source_id,
            deps,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlEntity::from(entity))
    }
}
