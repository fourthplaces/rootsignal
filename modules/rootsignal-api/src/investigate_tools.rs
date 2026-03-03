use std::sync::Arc;

use ai_client::tool::{Tool, ToolDefinition};
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use rootsignal_common::Node;
use rootsignal_graph::{GraphStore, PublicGraphReader};

use crate::db::scout_run::{self, EventRow, EventRowFull, json_str, event_layer, event_summary};

// ---------------------------------------------------------------------------
// Shared error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct ToolError(pub(crate) String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ToolError {}

// ---------------------------------------------------------------------------
// Shared output types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct EventSummary {
    seq: i64,
    ts: String,
    layer: String,
    name: String,
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
}

impl EventSummary {
    fn from_row(r: &EventRowFull, include_payload: bool) -> Self {
        let name = json_str(&r.data, "type").unwrap_or_else(|| r.event_type.clone());
        let summary = event_summary(&name, &r.data);
        let layer = event_layer(&r.event_type).to_string();
        Self {
            seq: r.seq,
            ts: r.ts.to_rfc3339(),
            layer,
            name,
            summary,
            run_id: r.run_id.clone(),
            payload: if include_payload { Some(r.data.clone()) } else { None },
        }
    }

    fn from_event_row(r: &EventRow) -> Self {
        let name = json_str(&r.data, "type").unwrap_or_else(|| r.event_type.clone());
        let summary = event_summary(&name, &r.data);
        let layer = event_layer(&r.event_type).to_string();
        Self {
            seq: r.seq,
            ts: r.ts.to_rfc3339(),
            layer,
            name,
            summary,
            run_id: None,
            payload: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 1. LoadCausalTreeTool
// ---------------------------------------------------------------------------

pub(crate) struct LoadCausalTreeTool {
    pub(crate) pool: Arc<PgPool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoadCausalTreeArgs {
    seq: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CausalTreeOutput {
    root_seq: i64,
    events: Vec<EventSummary>,
}

#[async_trait]
impl Tool for LoadCausalTreeTool {
    const NAME: &'static str = "load_causal_tree";
    type Error = ToolError;
    type Args = LoadCausalTreeArgs;
    type Output = CausalTreeOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Load the full causal chain for an event. Returns all events sharing the same correlation ID, ordered by sequence number.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "seq": { "type": "integer", "description": "The sequence number of the event to load the causal tree for" }
                },
                "required": ["seq"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (rows, root_seq) = scout_run::causal_tree(&self.pool, args.seq)
            .await
            .map_err(|e| ToolError(format!("Failed to load causal tree: {e}")))?;

        Ok(CausalTreeOutput {
            root_seq,
            events: rows.iter().map(|r| EventSummary::from_row(r, true)).collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// 2. SearchEventsTool
// ---------------------------------------------------------------------------

pub(crate) struct SearchEventsTool {
    pub(crate) pool: Arc<PgPool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchEventsArgs {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: i64,
}

fn default_search_limit() -> i64 { 20 }

#[derive(Debug, Serialize)]
pub(crate) struct SearchEventsOutput {
    events: Vec<EventSummary>,
}

#[async_trait]
impl Tool for SearchEventsTool {
    const NAME: &'static str = "search_events";
    type Error = ToolError;
    type Args = SearchEventsArgs;
    type Output = SearchEventsOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search events by keyword across payloads, event types, run IDs, and correlation IDs. Returns matching events in reverse chronological order.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search keyword or phrase" },
                    "limit": { "type": "integer", "description": "Max results to return (default 20, max 50)", "default": 20 }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let limit = args.limit.min(50);
        let rows = scout_run::list_events_paginated(&self.pool, Some(&args.query), None, None, None, None, limit)
            .await
            .map_err(|e| ToolError(format!("Search failed: {e}")))?;

        Ok(SearchEventsOutput {
            events: rows.iter().map(|r| EventSummary::from_row(r, false)).collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// 3. GetEventTool
// ---------------------------------------------------------------------------

pub(crate) struct GetEventTool {
    pub(crate) pool: Arc<PgPool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetEventArgs {
    seq: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct GetEventOutput {
    event: Option<EventSummary>,
}

#[async_trait]
impl Tool for GetEventTool {
    const NAME: &'static str = "get_event";
    type Error = ToolError;
    type Args = GetEventArgs;
    type Output = GetEventOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Load a single event's full payload by sequence number.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "seq": { "type": "integer", "description": "The sequence number of the event to load" }
                },
                "required": ["seq"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let row = scout_run::get_event_by_seq(&self.pool, args.seq)
            .await
            .map_err(|e| ToolError(format!("Failed to load event: {e}")))?;

        let event = row.as_ref().map(|r| EventSummary::from_row(r, true));

        Ok(GetEventOutput { event })
    }
}

// ---------------------------------------------------------------------------
// 4. GetSignalTool
// ---------------------------------------------------------------------------

pub(crate) struct GetSignalTool {
    pub(crate) reader: Arc<PublicGraphReader>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetSignalArgs {
    signal_id: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct SignalOutput {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    signal_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extracted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_confirmed_active: Option<String>,
}

fn node_to_signal_output(node: &Node) -> SignalOutput {
    match node.meta() {
        Some(meta) => SignalOutput {
            found: true,
            signal_type: Some(format!("{:?}", node.node_type())),
            title: Some(meta.title.clone()),
            summary: Some(meta.summary.clone()),
            confidence: Some(meta.confidence),
            category: meta.category.clone(),
            source_url: Some(meta.source_url.clone()),
            location_name: meta.about_location_name.clone(),
            review_status: Some(format!("{:?}", meta.review_status)),
            extracted_at: Some(meta.extracted_at.to_rfc3339()),
            published_at: meta.published_at.map(|t| t.to_rfc3339()),
            last_confirmed_active: Some(meta.last_confirmed_active.to_rfc3339()),
        },
        None => SignalOutput {
            found: true,
            signal_type: Some(format!("{:?}", node.node_type())),
            ..Default::default()
        },
    }
}

#[async_trait]
impl Tool for GetSignalTool {
    const NAME: &'static str = "get_signal";
    type Error = ToolError;
    type Args = GetSignalArgs;
    type Output = SignalOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Load a signal node from the graph by its UUID. Returns the signal type, title, summary, confidence, category, source URL, location, and temporal metadata (extracted_at, published_at, last_confirmed_active).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "signal_id": { "type": "string", "description": "The UUID of the signal to look up" }
                },
                "required": ["signal_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let uuid = uuid::Uuid::parse_str(&args.signal_id)
            .map_err(|e| ToolError(format!("Invalid UUID: {e}")))?;

        let node = self.reader.get_signal_by_id(uuid)
            .await
            .map_err(|e| ToolError(format!("Graph query failed: {e}")))?;

        match node {
            Some(n) => Ok(node_to_signal_output(&n)),
            None => Ok(SignalOutput::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// 5. FindEventsForNodeTool
// ---------------------------------------------------------------------------

pub(crate) struct FindEventsForNodeTool {
    pub(crate) pool: Arc<PgPool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FindEventsForNodeArgs {
    node_id: String,
    #[serde(default = "default_node_limit")]
    limit: u32,
}

fn default_node_limit() -> u32 { 20 }

#[derive(Debug, Serialize)]
pub(crate) struct FindEventsForNodeOutput {
    events: Vec<EventSummary>,
}

#[async_trait]
impl Tool for FindEventsForNodeTool {
    const NAME: &'static str = "find_events_for_node";
    type Error = ToolError;
    type Args = FindEventsForNodeArgs;
    type Output = FindEventsForNodeOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find events that created or modified a graph node (signal or source). Searches by node_id, matched_id, and existing_id in event payloads.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "The UUID of the graph node to search for" },
                    "limit": { "type": "integer", "description": "Max results (default 20, max 50)", "default": 20 }
                },
                "required": ["node_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let limit = args.limit.min(50);
        let rows = scout_run::list_events_by_node_id(&self.pool, &args.node_id, limit)
            .await
            .map_err(|e| ToolError(format!("Query failed: {e}")))?;

        Ok(FindEventsForNodeOutput {
            events: rows.iter().map(|r| EventSummary::from_event_row(r)).collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// 6. GetRunInfoTool
// ---------------------------------------------------------------------------

pub(crate) struct GetRunInfoTool {
    pub(crate) pool: Arc<PgPool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetRunInfoArgs {
    run_id: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RunInfoOutput {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    urls_scraped: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signals_extracted: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signals_stored: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signals_deduplicated: Option<u32>,
}

#[async_trait]
impl Tool for GetRunInfoTool {
    const NAME: &'static str = "get_run_info";
    type Error = ToolError;
    type Args = GetRunInfoArgs;
    type Output = RunInfoOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get metadata about a scout run including region, timing, and statistics (URLs scraped, signals extracted/stored/deduplicated).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "The run ID to look up" }
                },
                "required": ["run_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let row = scout_run::find_by_id(&self.pool, &args.run_id)
            .await
            .map_err(|e| ToolError(format!("Query failed: {e}")))?;

        match row {
            Some(r) => Ok(RunInfoOutput {
                found: true,
                run_id: Some(r.run_id),
                region: Some(r.region),
                started_at: Some(r.started_at.to_rfc3339()),
                finished_at: Some(r.finished_at.to_rfc3339()),
                urls_scraped: r.stats.urls_scraped,
                signals_extracted: r.stats.signals_extracted,
                signals_stored: r.stats.signals_stored,
                signals_deduplicated: r.stats.signals_deduplicated,
            }),
            None => Ok(RunInfoOutput::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// 7. CreateGitHubIssueTool
// ---------------------------------------------------------------------------

pub(crate) struct CreateGitHubIssueTool {
    pub(crate) github_token: String,
    pub(crate) github_repo: String, // "owner/repo"
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateGitHubIssueArgs {
    title: String,
    body: String,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateGitHubIssueOutput {
    issue_url: String,
    issue_number: u64,
}

#[derive(Deserialize)]
struct GitHubIssueResponse {
    html_url: String,
    number: u64,
}

#[async_trait]
impl Tool for CreateGitHubIssueTool {
    const NAME: &'static str = "create_github_issue";
    type Error = ToolError;
    type Args = CreateGitHubIssueArgs;
    type Output = CreateGitHubIssueOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create a GitHub issue on the project repository. This creates a REAL issue visible to the team — only call this after the user explicitly confirms they want an issue filed.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short, descriptive issue title" },
                    "body": { "type": "string", "description": "Issue body in Markdown. Include relevant event seqs, signal IDs, and a summary of what you found." },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional labels to apply (e.g. [\"bug\", \"investigation\"])"
                    }
                },
                "required": ["title", "body"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let url = format!("https://api.github.com/repos/{}/issues", self.github_repo);

        let mut payload = serde_json::json!({
            "title": args.title,
            "body": args.body,
        });
        if !args.labels.is_empty() {
            payload["labels"] = serde_json::json!(args.labels);
        }

        let resp = HttpClient::new()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.github_token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "rootsignal-investigator")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError(format!("GitHub API request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ToolError(format!("GitHub API returned {status}: {body}")));
        }

        let issue: GitHubIssueResponse = resp
            .json()
            .await
            .map_err(|e| ToolError(format!("Failed to parse GitHub response: {e}")))?;

        Ok(CreateGitHubIssueOutput {
            issue_url: issue.html_url,
            issue_number: issue.number,
        })
    }
}

// ---------------------------------------------------------------------------
// 8. FetchUrlTool
// ---------------------------------------------------------------------------

pub(crate) struct FetchUrlTool;

#[derive(Debug, Deserialize)]
pub(crate) struct FetchUrlArgs {
    url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FetchUrlOutput {
    status: u16,
    content_type: Option<String>,
    body: String,
    truncated: bool,
}

const FETCH_MAX_BYTES: usize = 10_000;

#[async_trait]
impl Tool for FetchUrlTool {
    const NAME: &'static str = "fetch_url";
    type Error = ToolError;
    type Args = FetchUrlArgs;
    type Output = FetchUrlOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetch a URL and return the page text. Useful for comparing what a source page actually says versus what was extracted. Returns up to 10KB of text content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The URL to fetch" }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resp = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ToolError(format!("Failed to create HTTP client: {e}")))?
            .get(&args.url)
            .header("User-Agent", "rootsignal-investigator/1.0")
            .send()
            .await
            .map_err(|e| ToolError(format!("Fetch failed: {e}")))?;

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let bytes = resp.bytes().await
            .map_err(|e| ToolError(format!("Failed to read response body: {e}")))?;

        let full_text = String::from_utf8_lossy(&bytes);
        let truncated = full_text.len() > FETCH_MAX_BYTES;
        let body = if truncated {
            full_text[..FETCH_MAX_BYTES].to_string()
        } else {
            full_text.into_owned()
        };

        Ok(FetchUrlOutput { status, content_type, body, truncated })
    }
}

// ---------------------------------------------------------------------------
// 9. GetFindingsForNodeTool
// ---------------------------------------------------------------------------

pub(crate) struct GetFindingsForNodeTool {
    pub(crate) reader: Arc<PublicGraphReader>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetFindingsForNodeArgs {
    node_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FindingOutput {
    id: String,
    issue_type: String,
    severity: String,
    description: String,
    suggested_action: String,
    status: String,
    created_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GetFindingsForNodeOutput {
    findings: Vec<FindingOutput>,
}

#[async_trait]
impl Tool for GetFindingsForNodeTool {
    const NAME: &'static str = "get_findings_for_node";
    type Error = ToolError;
    type Args = GetFindingsForNodeArgs;
    type Output = GetFindingsForNodeOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Check if the supervisor has flagged any quality issues for a specific signal or source node. Returns open and resolved findings.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "The UUID of the signal or source to check for supervisor findings" }
                },
                "required": ["node_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let rows = self.reader
            .list_validation_issues_for_target(&args.node_id, 20)
            .await
            .map_err(|e| ToolError(format!("Query failed: {e}")))?;

        let findings = rows.into_iter().map(|r| FindingOutput {
            id: r.id,
            issue_type: r.issue_type,
            severity: r.severity,
            description: r.description,
            suggested_action: r.suggested_action,
            status: r.status,
            created_at: r.created_at.map(|t| t.to_rfc3339()),
        }).collect();

        Ok(GetFindingsForNodeOutput { findings })
    }
}

// ---------------------------------------------------------------------------
// 10. GetSourceInfoTool
// ---------------------------------------------------------------------------

pub(crate) struct GetSourceInfoTool {
    pub(crate) writer: GraphStore,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetSourceInfoArgs {
    url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SourceInfoOutput {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quality_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signals_produced: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signals_corroborated: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    consecutive_empty_runs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avg_signals_per_scrape: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cadence_hours: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_scraped: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_produced_signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap_context: Option<String>,
}

#[async_trait]
impl Tool for GetSourceInfoTool {
    const NAME: &'static str = "get_source_info";
    type Error = ToolError;
    type Args = GetSourceInfoArgs;
    type Output = SourceInfoOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Look up source metadata by URL. Returns weight, quality penalty, effective weight, discovery method, scrape cadence, signal production stats, and active status.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The source URL to look up (substring match)" }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let sources = self.writer
            .search_sources(Some(&args.url))
            .await
            .map_err(|e| ToolError(format!("Query failed: {e}")))?;

        // Return the first match (search_sources does substring matching)
        match sources.first() {
            Some(s) => {
                let effective_weight = s.weight * s.quality_penalty;
                Ok(SourceInfoOutput {
                    found: true,
                    id: Some(s.id.to_string()),
                    canonical_value: Some(s.canonical_value.clone()),
                    discovery_method: Some(format!("{:?}", s.discovery_method)),
                    weight: Some(s.weight),
                    quality_penalty: Some(s.quality_penalty),
                    effective_weight: Some(effective_weight),
                    signals_produced: Some(s.signals_produced),
                    signals_corroborated: Some(s.signals_corroborated),
                    consecutive_empty_runs: Some(s.consecutive_empty_runs),
                    avg_signals_per_scrape: Some(s.avg_signals_per_scrape),
                    cadence_hours: s.cadence_hours,
                    last_scraped: s.last_scraped.map(|t| t.to_rfc3339()),
                    last_produced_signal: s.last_produced_signal.map(|t| t.to_rfc3339()),
                    active: Some(s.active),
                    gap_context: s.gap_context.clone(),
                })
            }
            None => Ok(SourceInfoOutput {
                found: false,
                id: None, canonical_value: None, discovery_method: None,
                weight: None, quality_penalty: None, effective_weight: None,
                signals_produced: None, signals_corroborated: None,
                consecutive_empty_runs: None, avg_signals_per_scrape: None,
                cadence_hours: None, last_scraped: None, last_produced_signal: None,
                active: None, gap_context: None,
            }),
        }
    }
}
