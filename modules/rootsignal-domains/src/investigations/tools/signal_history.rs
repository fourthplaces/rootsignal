use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct SignalHistoryArgs {
    pub entity_id: String,
}

#[derive(Debug, Serialize)]
pub struct SignalHistoryOutput {
    pub entity_id: String,
    pub first_seen: Option<String>,
    pub listing_count: i64,
    pub source_count: i64,
    pub days_active: Option<i64>,
}

pub struct InternalSignalHistoryTool {
    pool: PgPool,
}

impl InternalSignalHistoryTool {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug)]
pub struct SignalHistoryError(anyhow::Error);

impl std::fmt::Display for SignalHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SignalHistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[async_trait]
impl Tool for InternalSignalHistoryTool {
    const NAME: &'static str = "internal_signal_history";
    type Error = SignalHistoryError;
    type Args = SignalHistoryArgs;
    type Output = SignalHistoryOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Query Root Signal's internal database for signal history about an entity — how long we've tracked it, how many listings and sources reference it.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "The UUID of the entity to look up"
                    }
                },
                "required": ["entity_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let entity_id: Uuid = args
            .entity_id
            .parse()
            .map_err(|e: uuid::Error| SignalHistoryError(e.into()))?;

        let row = sqlx::query_as::<_, (Option<chrono::DateTime<chrono::Utc>>, i64)>(
            r#"
            SELECT MIN(l.created_at) as first_seen, COUNT(DISTINCT l.id) as listing_count
            FROM listings l
            WHERE l.entity_id = $1
            "#,
        )
        .bind(entity_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SignalHistoryError(e.into()))?;

        let source_count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(DISTINCT s.id) as source_count
            FROM sources s
            WHERE s.entity_id = $1
            "#,
        )
        .bind(entity_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SignalHistoryError(e.into()))?;

        let first_seen = row.0;
        let listing_count = row.1;
        let days_active = first_seen.map(|fs| (chrono::Utc::now() - fs).num_days());

        Ok(SignalHistoryOutput {
            entity_id: entity_id.to_string(),
            first_seen: first_seen.map(|dt| dt.to_rfc3339()),
            listing_count,
            source_count: source_count.0,
            days_active,
        })
    }
}
