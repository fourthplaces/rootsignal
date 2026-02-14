pub mod types;

use async_graphql::connection::*;
use async_graphql::*;
use uuid::Uuid;

use types::GqlEntity;

#[derive(Default)]
pub struct EntityQuery;

#[Object]
impl EntityQuery {
    async fn entity(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlEntity> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let entity = taproot_domains::entities::Entity::find_by_id(id, pool)
            .await
            .map_err(|_| async_graphql::Error::new(format!("entity {id} not found")))?;
        Ok(GqlEntity::from(entity))
    }

    #[graphql(complexity = "first.unwrap_or(20) as usize * child_complexity + 1")]
    async fn entities(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        first: Option<i32>,
    ) -> Result<Connection<String, GqlEntity>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        query(
            after,
            None::<String>,
            first,
            None::<i32>,
            |after: Option<String>, _before, first, _last| async move {
                let limit = first.unwrap_or(20).min(100) as i64;

                let rows = if let Some(ref cursor) = after {
                    let id: Uuid = cursor
                        .parse()
                        .map_err(|_| async_graphql::Error::new("invalid cursor"))?;
                    sqlx::query_as::<_, taproot_domains::entities::Entity>(
                        "SELECT * FROM entities WHERE id < $1 ORDER BY id DESC LIMIT $2",
                    )
                    .bind(id)
                    .bind(limit + 1)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?
                } else {
                    sqlx::query_as::<_, taproot_domains::entities::Entity>(
                        "SELECT * FROM entities ORDER BY id DESC LIMIT $1",
                    )
                    .bind(limit + 1)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?
                };

                let has_next = rows.len() as i64 > limit;
                let has_prev = after.is_some();
                let nodes: Vec<_> = rows.into_iter().take(limit as usize).collect();

                let mut connection = Connection::new(has_prev, has_next);
                connection.edges.extend(nodes.into_iter().map(|e| {
                    let cursor = e.id.to_string();
                    Edge::new(cursor, GqlEntity::from(e))
                }));

                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
    }
}
