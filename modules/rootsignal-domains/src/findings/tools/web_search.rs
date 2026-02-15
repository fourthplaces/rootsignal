use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use rootsignal_core::WebSearcher;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct WebSearchOutput {
    pub results: Vec<WebSearchResult>,
    pub result_count: usize,
}

#[derive(Debug, Serialize)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct FindingWebSearchTool {
    web_searcher: Arc<dyn WebSearcher>,
    pool: PgPool,
    investigation_id: Uuid,
}

impl FindingWebSearchTool {
    pub fn new(web_searcher: Arc<dyn WebSearcher>, pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            web_searcher,
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct WebSearchError(anyhow::Error);

impl std::fmt::Display for WebSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for WebSearchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[async_trait]
impl Tool for FindingWebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = WebSearchError;
    type Args = WebSearchArgs;
    type Output = WebSearchOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web for news, reports, and information about events or phenomena. Use specific queries about the topic you're investigating.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query — be specific about the topic, location, and timeframe"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| WebSearchError(e))?;

        let pages = self
            .web_searcher
            .search(&args.query, 5)
            .await
            .map_err(|e| WebSearchError(e))?;

        let results: Vec<WebSearchResult> = pages
            .into_iter()
            .map(|page| WebSearchResult {
                title: page.title.unwrap_or_default(),
                url: page.url.clone(),
                snippet: page.content.chars().take(500).collect(),
            })
            .collect();

        let result_count = results.len();

        InvestigationStep::create(
            self.investigation_id,
            step_number,
            Self::NAME,
            serde_json::json!({ "query": args.query }),
            serde_json::json!({ "result_count": result_count }),
            None,
            &self.pool,
        )
        .await
        .map_err(|e| WebSearchError(e))?;

        Ok(WebSearchOutput {
            results,
            result_count,
        })
    }
}
