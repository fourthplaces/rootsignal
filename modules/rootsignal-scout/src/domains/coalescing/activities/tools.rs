use std::sync::Arc;

use ai_client::tool::{Tool, ToolDefinition};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_graph::GraphQueries;
use crate::infra::embedder::TextEmbedder;

#[derive(Debug)]
pub struct ToolError(pub String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ToolError {}

// ---------------------------------------------------------------------------
// SearchSignals — fulltext search with embedding fallback
// ---------------------------------------------------------------------------

pub struct SearchSignalsTool {
    pub graph: Arc<dyn GraphQueries>,
    pub embedder: Arc<dyn TextEmbedder>,
}

#[derive(Debug, Deserialize)]
pub struct SearchSignalsArgs {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct SearchSignalsOutput {
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub signal_type: String,
    pub score: f64,
}

#[async_trait]
impl Tool for SearchSignalsTool {
    const NAME: &'static str = "search_signals";
    type Error = ToolError;
    type Args = SearchSignalsArgs;
    type Output = SearchSignalsOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search for signals in the community database by keyword or phrase. \
                Returns matching signals with titles, summaries, and relevance scores."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query — use community language, not technical jargon"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        const MAX_RESULTS: u32 = 20;
        const FULLTEXT_MIN: usize = 3;

        let fulltext = self
            .graph
            .fulltext_search_signals(&args.query, MAX_RESULTS)
            .await
            .map_err(|e| ToolError(format!("Fulltext search failed: {e}")))?;

        let results = if fulltext.len() >= FULLTEXT_MIN {
            fulltext
        } else {
            let embedding = self
                .embedder
                .embed(&args.query)
                .await
                .map_err(|e| ToolError(format!("Embedding failed: {e}")))?;
            self.graph
                .vector_search_signals(&embedding, MAX_RESULTS, None)
                .await
                .map_err(|e| ToolError(format!("Vector search failed: {e}")))?
        };

        let items = results
            .into_iter()
            .map(|r| SearchResultItem {
                id: r.id.to_string(),
                title: r.title,
                summary: r.summary,
                signal_type: r.signal_type,
                score: r.score,
            })
            .collect();

        Ok(SearchSignalsOutput { results: items })
    }
}

// ---------------------------------------------------------------------------
// FindSimilar — vector similarity from a known signal
// ---------------------------------------------------------------------------

pub struct FindSimilarTool {
    pub graph: Arc<dyn GraphQueries>,
}

#[derive(Debug, Deserialize)]
pub struct FindSimilarArgs {
    pub signal_id: String,
}

#[derive(Debug, Serialize)]
pub struct FindSimilarOutput {
    pub results: Vec<SearchResultItem>,
}

#[async_trait]
impl Tool for FindSimilarTool {
    const NAME: &'static str = "find_similar";
    type Error = ToolError;
    type Args = FindSimilarArgs;
    type Output = FindSimilarOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find signals similar to a given signal. \
                Returns signals ranked by similarity."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "signal_id": {
                        "type": "string",
                        "description": "The UUID of the signal to find similar signals for"
                    }
                },
                "required": ["signal_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let signal_id = Uuid::parse_str(&args.signal_id)
            .map_err(|_| ToolError(format!("Invalid signal ID: {}", args.signal_id)))?;

        let details = self
            .graph
            .get_signal_details(&[signal_id])
            .await
            .map_err(|e| ToolError(format!("Failed to fetch signal: {e}")))?;

        let signal = details
            .first()
            .ok_or_else(|| ToolError(format!("Signal {} not found", signal_id)))?;

        // Use the signal's title + summary as a semantic query via fulltext
        let query_text = format!("{} {}", signal.title, signal.summary);
        let results = self
            .graph
            .fulltext_search_signals(&query_text, 20)
            .await
            .map_err(|e| ToolError(format!("Search failed: {e}")))?;

        let items: Vec<SearchResultItem> = results
            .into_iter()
            .filter(|r| r.id != signal_id)
            .map(|r| SearchResultItem {
                id: r.id.to_string(),
                title: r.title,
                summary: r.summary,
                signal_type: r.signal_type,
                score: r.score,
            })
            .collect();

        Ok(FindSimilarOutput { results: items })
    }
}
