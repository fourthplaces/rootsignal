pub mod types;

use async_graphql::*;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::observations::types::GqlInvestigation;
use rootsignal_domains::investigations::Investigation;

#[derive(Default)]
pub struct InvestigationQuery;

#[Object]
impl InvestigationQuery {
    /// List investigations with optional status filter and pagination.
    async fn investigations(
        &self,
        ctx: &Context<'_>,
        status: Option<String>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> Result<Vec<GqlInvestigation>> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let limit = limit.min(100) as i64;
        let offset = offset.max(0) as i64;

        let investigations = if let Some(ref status) = status {
            sqlx::query_as::<_, Investigation>(
                "SELECT * FROM investigations WHERE status = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(status)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, Investigation>(
                "SELECT * FROM investigations ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        };

        Ok(investigations
            .into_iter()
            .map(GqlInvestigation::from)
            .collect())
    }

    /// Count investigations, optionally by status.
    async fn investigation_count(
        &self,
        ctx: &Context<'_>,
        status: Option<String>,
    ) -> Result<i64> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let count = if let Some(ref status) = status {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM investigations WHERE status = $1",
            )
            .bind(status)
            .fetch_one(pool)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM investigations")
                .fetch_one(pool)
                .await?
        };

        Ok(count)
    }
}
