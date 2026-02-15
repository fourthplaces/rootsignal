pub mod mutations;
pub mod types;

use async_graphql::*;
use uuid::Uuid;

use crate::graphql::error;
use rootsignal_domains::findings::{Connection, Finding};
use types::{FindingStatus, GqlConnection, GqlFinding, GqlFindingConnection};

#[derive(Default)]
pub struct FindingQuery;

#[Object]
impl FindingQuery {
    /// Fetch a single finding by ID.
    async fn finding(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlFinding> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let finding = Finding::find_by_id(id, pool)
            .await
            .map_err(|_| error::not_found(format!("finding {id}")))?;
        Ok(GqlFinding::from(finding))
    }

    /// Query findings with optional filters.
    async fn findings(
        &self,
        ctx: &Context<'_>,
        status: Option<FindingStatus>,
        search: Option<String>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> Result<GqlFindingConnection> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let limit = limit.min(100) as i64;
        let offset = offset.max(0) as i64;

        if let Some(ref query) = search {
            let status_str = status.map(|s| s.as_str());
            let nodes: Vec<GqlFinding> =
                Finding::search(query, status_str, limit, offset, pool)
                    .await?
                    .into_iter()
                    .map(GqlFinding::from)
                    .collect();
            let total = nodes.len() as i64;
            return Ok(GqlFindingConnection {
                nodes,
                total_count: total,
            });
        }

        if let Some(status) = status {
            let nodes: Vec<GqlFinding> =
                Finding::find_by_status(status.as_str(), limit, offset, pool)
                    .await?
                    .into_iter()
                    .map(GqlFinding::from)
                    .collect();
            let total = Finding::count_by_status(status.as_str(), pool).await?;
            return Ok(GqlFindingConnection {
                nodes,
                total_count: total,
            });
        }

        let nodes: Vec<GqlFinding> = Finding::find_all(limit, offset, pool)
            .await?
            .into_iter()
            .map(GqlFinding::from)
            .collect();
        let total = Finding::count(pool).await?;
        Ok(GqlFindingConnection {
            nodes,
            total_count: total,
        })
    }

    /// Query connections for a finding, optionally filtered by role.
    async fn connections(
        &self,
        ctx: &Context<'_>,
        finding_id: Uuid,
        role: Option<String>,
    ) -> Result<Vec<GqlConnection>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let connections = if let Some(ref role) = role {
            Connection::find_to_by_role("finding", finding_id, role, pool).await?
        } else {
            Connection::find_to("finding", finding_id, pool).await?
        };
        Ok(connections.into_iter().map(GqlConnection::from).collect())
    }
}
