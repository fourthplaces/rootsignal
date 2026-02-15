use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use rootsignal_core::Ingestor;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct FollowLinkArgs {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct FollowLinkOutput {
    pub url: String,
    pub title: Option<String>,
    pub content_preview: String,
    pub page_snapshot_id: Option<String>,
}

pub struct FollowLinkTool {
    ingestor: Arc<dyn Ingestor>,
    pool: PgPool,
    investigation_id: Uuid,
}

impl FollowLinkTool {
    pub fn new(ingestor: Arc<dyn Ingestor>, pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            ingestor,
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct FollowLinkError(anyhow::Error);

impl std::fmt::Display for FollowLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FollowLinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[async_trait]
impl Tool for FollowLinkTool {
    const NAME: &'static str = "follow_link";
    type Error = FollowLinkError;
    type Args = FollowLinkArgs;
    type Output = FollowLinkOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetch and read a web page. Returns the page content. Use this to follow links found in evidence or signals.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| FollowLinkError(e))?;

        let page = self
            .ingestor
            .fetch_one(&args.url)
            .await
            .map_err(|e| FollowLinkError(e.into()))?;

        let content_preview: String = page.content.chars().take(2000).collect();

        // Store as page snapshot if we have content
        let snapshot_id = if page.has_content() {
            let sid = sqlx::query_as::<_, (Uuid,)>(
                r#"
                INSERT INTO page_snapshots (url, canonical_url, content_hash, raw_content, html, fetched_via, crawled_at)
                VALUES ($1, $1, $2, $3, $4, 'investigation', NOW())
                ON CONFLICT (canonical_url, content_hash) DO UPDATE SET url = EXCLUDED.url
                RETURNING id
                "#,
            )
            .bind(&page.url)
            .bind(&page.content_hash())
            .bind(&page.content)
            .bind(&page.html)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| FollowLinkError(e.into()))?;
            Some(sid.0)
        } else {
            None
        };

        // Log step
        InvestigationStep::create(
            self.investigation_id,
            step_number,
            Self::NAME,
            serde_json::json!({ "url": args.url }),
            serde_json::json!({
                "content_length": page.content.len(),
                "title": page.title,
            }),
            snapshot_id,
            &self.pool,
        )
        .await
        .map_err(|e| FollowLinkError(e))?;

        Ok(FollowLinkOutput {
            url: page.url,
            title: page.title,
            content_preview,
            page_snapshot_id: snapshot_id.map(|id| id.to_string()),
        })
    }
}
