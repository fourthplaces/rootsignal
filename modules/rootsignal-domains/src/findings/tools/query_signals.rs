use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct QuerySignalsArgs {
    pub search: Option<String>,
    pub signal_type: Option<String>,
    pub city: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QuerySignalsOutput {
    pub signals: Vec<SignalSummary>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SignalSummary {
    pub id: String,
    pub signal_type: String,
    pub content: String,
    pub about: Option<String>,
    pub source_url: Option<String>,
    pub created_at: String,
}

pub struct QuerySignalsTool {
    pool: PgPool,
    investigation_id: Uuid,
}

impl QuerySignalsTool {
    pub fn new(pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct QuerySignalsError(anyhow::Error);

impl std::fmt::Display for QuerySignalsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for QuerySignalsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SignalRow {
    id: Uuid,
    signal_type: String,
    content: String,
    about: Option<String>,
    source_url: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl Tool for QuerySignalsTool {
    const NAME: &'static str = "query_signals";
    type Error = QuerySignalsError;
    type Args = QuerySignalsArgs;
    type Output = QuerySignalsOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Query existing signals in our database. Search by text, filter by type (ask/give/event/informative) and city. Use this to find related signals that may be part of the same phenomenon.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "search": {
                        "type": "string",
                        "description": "Full-text search query"
                    },
                    "signal_type": {
                        "type": "string",
                        "description": "Filter by signal type: ask, give, event, informative"
                    },
                    "city": {
                        "type": "string",
                        "description": "Filter by city name"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| QuerySignalsError(e))?;

        let rows = if let Some(ref search) = args.search {
            sqlx::query_as::<_, SignalRow>(
                r#"
                SELECT s.id, s.signal_type, s.content, s.about, s.source_url, s.created_at
                FROM signals s
                WHERE s.search_vector @@ plainto_tsquery('english', $1)
                  AND ($2::text IS NULL OR s.signal_type = $2)
                ORDER BY ts_rank(s.search_vector, plainto_tsquery('english', $1)) DESC
                LIMIT 20
                "#,
            )
            .bind(search)
            .bind(args.signal_type.as_deref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| QuerySignalsError(e.into()))?
        } else if let Some(ref city) = args.city {
            sqlx::query_as::<_, SignalRow>(
                r#"
                SELECT DISTINCT s.id, s.signal_type, s.content, s.about, s.source_url, s.created_at
                FROM signals s
                JOIN locationables la ON la.locatable_type = 'signal' AND la.locatable_id = s.id
                JOIN locations l ON l.id = la.location_id
                WHERE LOWER(l.city) = LOWER($1)
                  AND ($2::text IS NULL OR s.signal_type = $2)
                ORDER BY s.created_at DESC
                LIMIT 20
                "#,
            )
            .bind(city)
            .bind(args.signal_type.as_deref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| QuerySignalsError(e.into()))?
        } else {
            sqlx::query_as::<_, SignalRow>(
                r#"
                SELECT s.id, s.signal_type, s.content, s.about, s.source_url, s.created_at
                FROM signals s
                WHERE ($1::text IS NULL OR s.signal_type = $1)
                ORDER BY s.created_at DESC
                LIMIT 20
                "#,
            )
            .bind(args.signal_type.as_deref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| QuerySignalsError(e.into()))?
        };

        let signals: Vec<SignalSummary> = rows
            .into_iter()
            .map(|r| SignalSummary {
                id: r.id.to_string(),
                signal_type: r.signal_type,
                content: r.content,
                about: r.about,
                source_url: r.source_url,
                created_at: r.created_at.to_rfc3339(),
            })
            .collect();

        let count = signals.len();

        InvestigationStep::create(
            self.investigation_id,
            step_number,
            Self::NAME,
            serde_json::json!({
                "search": args.search,
                "signal_type": args.signal_type,
                "city": args.city,
            }),
            serde_json::json!({ "count": count }),
            None,
            &self.pool,
        )
        .await
        .map_err(|e| QuerySignalsError(e))?;

        Ok(QuerySignalsOutput { signals, count })
    }
}
