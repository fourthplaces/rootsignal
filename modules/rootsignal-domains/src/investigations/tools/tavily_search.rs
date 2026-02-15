use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use rootsignal_core::WebSearcher;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct TavilyEntitySearchArgs {
    pub entity_name: String,
    pub location: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TavilyEntitySearchOutput {
    pub results: Vec<SearchResult>,
    pub result_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct TavilyEntitySearchTool {
    web_searcher: Arc<dyn WebSearcher>,
}

impl TavilyEntitySearchTool {
    pub fn new(web_searcher: Arc<dyn WebSearcher>) -> Self {
        Self { web_searcher }
    }
}

#[derive(Debug)]
pub struct TavilySearchError(anyhow::Error);

impl std::fmt::Display for TavilySearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TavilySearchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[async_trait]
impl Tool for TavilyEntitySearchTool {
    const NAME: &'static str = "tavily_entity_search";
    type Error = TavilySearchError;
    type Args = TavilyEntitySearchArgs;
    type Output = TavilyEntitySearchOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web for information about an entity to assess its real-world presence and legitimacy.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_name": {
                        "type": "string",
                        "description": "The name of the entity to search for"
                    },
                    "location": {
                        "type": "string",
                        "description": "Optional location to narrow the search (e.g. 'Portland, OR')"
                    }
                },
                "required": ["entity_name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let query = match args.location {
            Some(ref loc) => format!("{} {}", args.entity_name, loc),
            None => args.entity_name.clone(),
        };

        let pages = self
            .web_searcher
            .search(&query, 5)
            .await
            .map_err(|e| TavilySearchError(e))?;

        let results: Vec<SearchResult> = pages
            .into_iter()
            .map(|page| SearchResult {
                title: page.title.unwrap_or_default(),
                url: page.url.clone(),
                snippet: page.content.chars().take(500).collect(),
            })
            .collect();

        let result_count = results.len();

        Ok(TavilyEntitySearchOutput {
            results,
            result_count,
        })
    }
}
