use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct QueryEntitiesArgs {
    pub name: Option<String>,
    pub entity_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QueryEntitiesOutput {
    pub entities: Vec<EntitySummary>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct EntitySummary {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub description: Option<String>,
    pub website: Option<String>,
}

pub struct QueryEntitiesTool {
    pool: PgPool,
    investigation_id: Uuid,
}

impl QueryEntitiesTool {
    pub fn new(pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct QueryEntitiesError(anyhow::Error);

impl std::fmt::Display for QueryEntitiesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for QueryEntitiesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[derive(Debug, sqlx::FromRow)]
struct EntityRow {
    id: Uuid,
    name: String,
    entity_type: String,
    description: Option<String>,
    website: Option<String>,
}

#[async_trait]
impl Tool for QueryEntitiesTool {
    const NAME: &'static str = "query_entities";
    type Error = QueryEntitiesError;
    type Args = QueryEntitiesArgs;
    type Output = QueryEntitiesOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Look up entities (organizations, government bodies, businesses) in our database. Search by name or filter by type.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Entity name to search for (partial match)"
                    },
                    "entity_type": {
                        "type": "string",
                        "description": "Filter by entity type: organization, government_entity, business_entity"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| QueryEntitiesError(e))?;

        let rows = if let Some(ref name) = args.name {
            sqlx::query_as::<_, EntityRow>(
                r#"
                SELECT id, name, entity_type, description, website
                FROM entities
                WHERE LOWER(name) LIKE '%' || LOWER($1) || '%'
                  AND ($2::text IS NULL OR entity_type = $2)
                ORDER BY name
                LIMIT 20
                "#,
            )
            .bind(name)
            .bind(args.entity_type.as_deref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| QueryEntitiesError(e.into()))?
        } else {
            sqlx::query_as::<_, EntityRow>(
                r#"
                SELECT id, name, entity_type, description, website
                FROM entities
                WHERE ($1::text IS NULL OR entity_type = $1)
                ORDER BY name
                LIMIT 20
                "#,
            )
            .bind(args.entity_type.as_deref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| QueryEntitiesError(e.into()))?
        };

        let entities: Vec<EntitySummary> = rows
            .into_iter()
            .map(|r| EntitySummary {
                id: r.id.to_string(),
                name: r.name,
                entity_type: r.entity_type,
                description: r.description,
                website: r.website,
            })
            .collect();

        let count = entities.len();

        InvestigationStep::create(
            self.investigation_id,
            step_number,
            Self::NAME,
            serde_json::json!({
                "name": args.name,
                "entity_type": args.entity_type,
            }),
            serde_json::json!({ "count": count }),
            None,
            &self.pool,
        )
        .await
        .map_err(|e| QueryEntitiesError(e))?;

        Ok(QueryEntitiesOutput { entities, count })
    }
}
