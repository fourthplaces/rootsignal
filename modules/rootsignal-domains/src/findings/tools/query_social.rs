use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct QuerySocialArgs {
    pub search: String,
}

#[derive(Debug, Serialize)]
pub struct QuerySocialOutput {
    pub posts: Vec<SocialPost>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SocialPost {
    pub url: String,
    pub content_preview: String,
    pub crawled_at: String,
}

pub struct QuerySocialTool {
    pool: PgPool,
    investigation_id: Uuid,
}

impl QuerySocialTool {
    pub fn new(pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct QuerySocialError(anyhow::Error);

impl std::fmt::Display for QuerySocialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for QuerySocialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SocialRow {
    url: String,
    raw_content: Option<String>,
    crawled_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl Tool for QuerySocialTool {
    const NAME: &'static str = "query_social";
    type Error = QuerySocialError;
    type Args = QuerySocialArgs;
    type Output = QuerySocialOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search already-captured social media posts and community content in our page snapshot database. Useful for finding firsthand accounts.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "search": {
                        "type": "string",
                        "description": "Search query for social media content"
                    }
                },
                "required": ["search"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| QuerySocialError(e))?;

        // Search page snapshots from social media sources
        let rows = sqlx::query_as::<_, SocialRow>(
            r#"
            SELECT ps.url, ps.raw_content, ps.crawled_at
            FROM page_snapshots ps
            JOIN domain_snapshots ds ON ds.page_snapshot_id = ps.id
            JOIN sources s ON s.id = ds.source_id
            WHERE (s.url LIKE '%facebook%' OR s.url LIKE '%twitter%' OR s.url LIKE '%x.com%'
                   OR s.url LIKE '%instagram%' OR s.url LIKE '%nextdoor%' OR s.url LIKE '%reddit%')
              AND ps.raw_content IS NOT NULL
              AND to_tsvector('english', ps.raw_content) @@ plainto_tsquery('english', $1)
            ORDER BY ps.crawled_at DESC
            LIMIT 10
            "#,
        )
        .bind(&args.search)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| QuerySocialError(e.into()))?;

        let posts: Vec<SocialPost> = rows
            .into_iter()
            .map(|r| SocialPost {
                url: r.url,
                content_preview: r
                    .raw_content
                    .unwrap_or_default()
                    .chars()
                    .take(500)
                    .collect(),
                crawled_at: r.crawled_at.to_rfc3339(),
            })
            .collect();

        let count = posts.len();

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
        .map_err(|e| QuerySocialError(e))?;

        Ok(QuerySocialOutput { posts, count })
    }
}
