pub mod types;

use async_graphql::*;
use types::{GqlTag, GqlTagKind};

#[derive(Default)]
pub struct TagQuery;

#[Object]
impl TagQuery {
    /// List tags, optionally filtered by kind (e.g., "signal_domain", "category").
    async fn tags(&self, ctx: &Context<'_>, kind: Option<String>) -> Result<Vec<GqlTag>> {
        tracing::info!(kind = ?kind, "graphql.tags");
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let tags = if let Some(kind) = kind {
            rootsignal_domains::taxonomy::Tag::find_by_kind(&kind, pool)
                .await
                .unwrap_or_default()
        } else {
            sqlx::query_as::<_, rootsignal_domains::taxonomy::Tag>(
                "SELECT * FROM tags ORDER BY kind, value",
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        };
        Ok(tags.into_iter().map(GqlTag::from).collect())
    }

    /// List all tag kind configurations (taxonomy metadata).
    async fn tag_kinds(&self, ctx: &Context<'_>) -> Result<Vec<GqlTagKind>> {
        tracing::info!("graphql.tag_kinds");
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let kinds = rootsignal_domains::taxonomy::TagKindConfig::find_all(pool)
            .await
            .unwrap_or_default();
        Ok(kinds.into_iter().map(GqlTagKind::from).collect())
    }
}
