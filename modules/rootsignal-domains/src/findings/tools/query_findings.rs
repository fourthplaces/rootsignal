use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct QueryFindingsArgs {
    pub search: String,
}

#[derive(Debug, Serialize)]
pub struct QueryFindingsOutput {
    pub findings: Vec<FindingSummary>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct FindingSummary {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub created_at: String,
}

pub struct QueryFindingsTool {
    pool: PgPool,
    investigation_id: Uuid,
}

impl QueryFindingsTool {
    pub fn new(pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct QueryFindingsError(anyhow::Error);

impl std::fmt::Display for QueryFindingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for QueryFindingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[derive(Debug, sqlx::FromRow)]
struct FindingRow {
    id: Uuid,
    title: String,
    summary: String,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl Tool for QueryFindingsTool {
    const NAME: &'static str = "query_findings";
    type Error = QueryFindingsError;
    type Args = QueryFindingsArgs;
    type Output = QueryFindingsOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search existing Findings by text. Use this to check if a broader phenomenon you've discovered already has a Finding, so you can propose a 'driven_by' connection instead of creating a duplicate.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "search": {
                        "type": "string",
                        "description": "Search query to find related findings"
                    }
                },
                "required": ["search"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| QueryFindingsError(e))?;

        let rows = sqlx::query_as::<_, FindingRow>(
            r#"
            SELECT id, title, summary, status, created_at
            FROM findings
            WHERE search_vector @@ plainto_tsquery('english', $1)
            ORDER BY ts_rank(search_vector, plainto_tsquery('english', $1)) DESC
            LIMIT 10
            "#,
        )
        .bind(&args.search)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| QueryFindingsError(e.into()))?;

        let findings: Vec<FindingSummary> = rows
            .into_iter()
            .map(|r| FindingSummary {
                id: r.id.to_string(),
                title: r.title,
                summary: r.summary,
                status: r.status,
                created_at: r.created_at.to_rfc3339(),
            })
            .collect();

        let count = findings.len();

        InvestigationStep::create(
            self.investigation_id,
            step_number,
            Self::NAME,
            serde_json::json!({ "search": args.search }),
            serde_json::json!({ "count": count }),
            None,
            &self.pool,
        )
        .await
        .map_err(|e| QueryFindingsError(e))?;

        Ok(QueryFindingsOutput { findings, count })
    }
}
