use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use ai_client::tool::{Tool, ToolDefinition};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use rootsignal_archive::Archive;

use crate::infra::run_log::{EventKind, EventLogger, RunLogger};

pub(crate) struct WebSearchTool {
    pub(crate) archive: Arc<Archive>,
    pub(crate) run_log: Option<RunLogger>,
    pub(crate) agent_name: String,
    pub(crate) tension_title: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebSearchArgs {
    pub(crate) query: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebSearchOutput {
    pub(crate) results: Vec<WebSearchResultItem>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebSearchResultItem {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) snippet: String,
}

#[derive(Debug)]
pub(crate) struct ToolError(pub(crate) String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ToolError {}

#[async_trait]
impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = ToolError;
    type Args = WebSearchArgs;
    type Output = WebSearchOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web for information. Returns URLs, titles, and snippets."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let handle = self.archive.source(&args.query).await
            .map_err(|e| ToolError(format!("Search failed: {e}")))?;
        let search = handle.search(&args.query).max_results(10).await
            .map_err(|e| ToolError(format!("Search failed: {e}")))?;

        let results: Vec<WebSearchResultItem> = search
            .results
            .into_iter()
            .map(|r| WebSearchResultItem {
                url: r.url,
                title: r.title,
                snippet: r.snippet,
            })
            .collect();

        if let Some(ref log) = self.run_log {
            log.log(EventKind::AgentWebSearch {
                provider: self.agent_name.clone(),
                query: args.query,
                result_count: results.len() as u32,
                title: self.tension_title.clone(),
            });
        }

        Ok(WebSearchOutput { results })
    }
}

pub(crate) struct ReadPageTool {
    pub(crate) archive: Arc<Archive>,
    /// When set, records every URL successfully read for post-hoc validation.
    pub(crate) visited_urls: Option<Arc<Mutex<HashSet<String>>>>,
    pub(crate) run_log: Option<RunLogger>,
    pub(crate) agent_name: String,
    pub(crate) tension_title: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadPageArgs {
    pub(crate) url: String,
}

#[async_trait]
impl Tool for ReadPageTool {
    const NAME: &'static str = "read_page";
    type Error = ToolError;
    type Args = ReadPageArgs;
    type Output = String;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description:
                "Read the full content of a web page. Returns the page as clean markdown text."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to read"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let page = self.archive.page(&args.url).await
            .map_err(|e| ToolError(format!("Scrape failed: {e}")))?;
        let content = page.markdown;

        // Record this URL as successfully visited
        if let Some(ref visited) = self.visited_urls {
            if let Ok(mut set) = visited.lock() {
                set.insert(args.url.clone());
            }
        }

        if let Some(ref log) = self.run_log {
            log.log(EventKind::AgentPageRead {
                provider: self.agent_name.clone(),
                url: args.url,
                content_chars: content.len(),
                title: self.tension_title.clone(),
            });
        }

        // Truncate to ~8k chars to fit in context
        let max_len = 8000;
        if content.len() > max_len {
            let mut end = max_len;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            Ok(format!(
                "{}...\n\n[Content truncated at {} chars]",
                &content[..end],
                max_len
            ))
        } else {
            Ok(content)
        }
    }
}
