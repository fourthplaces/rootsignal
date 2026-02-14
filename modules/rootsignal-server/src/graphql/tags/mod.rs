pub mod types;

use async_graphql::*;
use types::{GqlTag, GqlTagKind};

#[derive(Default)]
pub struct TagQuery;

#[Object]
impl TagQuery {
    /// List tags, optionally filtered by kind (e.g., "listing_type", "category").
    async fn tags(&self, ctx: &Context<'_>, kind: Option<String>) -> Result<Vec<GqlTag>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let tags = if let Some(kind) = kind {
            rootsignal_domains::entities::Tag::find_by_kind(&kind, pool)
                .await
                .unwrap_or_default()
        } else {
            sqlx::query_as::<_, rootsignal_domains::entities::Tag>(
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
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let kinds = rootsignal_domains::entities::TagKindConfig::find_all(pool)
            .await
            .unwrap_or_default();
        Ok(kinds.into_iter().map(GqlTagKind::from).collect())
    }
}
