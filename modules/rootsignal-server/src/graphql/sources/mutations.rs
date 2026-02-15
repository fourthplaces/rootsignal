use async_graphql::*;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;
use super::types::GqlSource;

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
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let config = input.config.unwrap_or(serde_json::Value::Object(Default::default()));

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

        Ok(GqlSource::from(source))
    }
}
