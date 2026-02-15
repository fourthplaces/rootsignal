pub mod mutations;
pub mod types;

use async_graphql::*;
use uuid::Uuid;
use types::GqlSource;

#[derive(Default)]
pub struct SourceQuery;

#[Object]
impl SourceQuery {
    async fn sources(&self, ctx: &Context<'_>) -> Result<Vec<GqlSource>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let sources = rootsignal_domains::scraping::Source::find_all(pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?;
        Ok(sources.into_iter().map(GqlSource::from).collect())
    }

    async fn source(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlSource> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let source = rootsignal_domains::scraping::Source::find_by_id(id, pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("source not found: {e}")))?;
        Ok(GqlSource::from(source))
    }
}
