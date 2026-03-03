use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use serde::Deserialize;

use ai_client::{Agent, Claude, Message, PromptBuilder};

use crate::db::scout_run::{self, event_layer, event_summary, json_str};
use crate::investigate_tools::{
    CreateGitHubIssueTool, FindEventsForNodeTool, GetEventTool, GetRunInfoTool, GetSignalTool,
    LoadCausalTreeTool, SearchEventsTool,
};
use crate::jwt;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct InvestigateRequest {
    seq: i64,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    role: String,
    content: String,
}

const SYSTEM_PROMPT: &str = r#"You are an event investigation assistant for RootSignal, an event-sourced community intelligence platform. Your job is to help operators understand what happened and why by analyzing events and their causal chains.

## Event Architecture

Events flow through a three-layer taxonomy:
- **World** (blue): Facts about reality — things discovered, scraped, extracted. Examples: source_discovered, gathering_extracted, concern_raised.
- **System** (amber): Decisions the system made — classifications, corrections, enrichments. Examples: category_classified, gathering_corrected.
- **Telemetry** (gray): Pipeline bookkeeping — run started/completed, scrape attempted. Examples: run_started, scrape_completed.

## Engine Dispatch Loop

Each event is processed through: PERSIST → REDUCE → ROUTE → RECURSE
- PERSIST: Event is stored in the event store (Postgres)
- REDUCE: State is updated based on the event
- ROUTE: Handlers decide what to do next, possibly emitting new events
- RECURSE: New events re-enter the loop

## Signal Types

The system extracts signals from web content:
- **Gathering**: Community events (meetups, festivals, town halls)
- **Resource**: Available help (food banks, legal aid, shelters)
- **HelpRequest**: Someone asking for help
- **Announcement**: General community announcements
- **Concern**: Friction, tensions, or issues in the community
- **Condition**: Ongoing environmental or social conditions

## Your Tools

You have tools to query the event store and signal graph. Use them to investigate thoroughly:
- The causal tree is already loaded for you in context — analyze it first
- `search_events` — find related events (same source, similar patterns)
- `get_signal` — inspect signal nodes referenced in event payloads (look for node_id, signal_id fields)
- `find_events_for_node` — trace what events touched a specific signal
- `get_run_info` — understand the run that produced this event
- `get_event` — load full payloads of specific events you want to inspect

## How to Respond

Talk like you're explaining what happened to a colleague. Be conversational and direct — tell the story of what happened, why, and what it triggered. Mention seq numbers naturally when they help ("that kicked off event 4231, which...") but don't build tables or numbered checklists unless the user asks for them.

Keep it concise. If something is interesting or unusual, say so. If the user wants a step-by-step timeline or structured breakdown, they'll ask in a follow-up.

If you spot something that looks like a bug or warrants a ticket, say so and offer to file a GitHub issue. Only call the `create_github_issue` tool after the user explicitly confirms — never create issues unprompted.
"#;

fn build_context(event: &scout_run::EventRowFull, tree: &[scout_run::EventRowFull]) -> String {
    let name = json_str(&event.data, "type").unwrap_or_else(|| event.event_type.clone());
    let summary = event_summary(&name, &event.data);
    let layer = event_layer(&event.event_type);
    let payload = serde_json::to_string_pretty(&event.data).unwrap_or_default();

    let mut ctx = format!(
        "## Selected Event\n\nseq={} | {} | {} | {}\n",
        event.seq, layer, name, event.ts,
    );
    if let Some(s) = &summary {
        ctx.push_str(&format!("Summary: {s}\n"));
    }
    ctx.push_str(&format!("\nPayload:\n```json\n{payload}\n```\n"));

    if !tree.is_empty() {
        ctx.push_str("\n## Causal Tree\n\n");
        ctx.push_str("| seq | layer | name | timestamp | summary |\n");
        ctx.push_str("|-----|-------|------|-----------|----------|\n");
        for e in tree {
            let n = json_str(&e.data, "type").unwrap_or_else(|| e.event_type.clone());
            let s = event_summary(&n, &e.data);
            let l = event_layer(&e.event_type);
            ctx.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                e.seq,
                l,
                n,
                e.ts,
                s.as_deref().unwrap_or("-"),
            ));
        }
    }

    ctx
}

pub async fn investigate_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: Result<Json<InvestigateRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let Json(body) = match body {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "Invalid investigate request body");
            return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
        }
    };

    // Auth: verify JWT cookie
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let claims = jwt::parse_auth_cookie(cookie_header)
        .and_then(|token| state.jwt_service.verify_token(token).ok());

    if claims.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let api_key = &state.config.anthropic_api_key;
    if api_key.is_empty() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    // Require Postgres for investigation tools
    let pool = match &state.pg_pool {
        Some(p) => Arc::new(p.clone()),
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "Database not available").into_response();
        }
    };

    // Load the selected event and its causal tree from the database
    let (tree_rows, _root_seq) = match scout_run::causal_tree(&pool, body.seq).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, seq = body.seq, "Failed to load causal tree");
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to load event: {e}")).into_response();
        }
    };

    // Find the selected event in the tree, or load it separately
    let selected_event = tree_rows.iter().find(|r| r.seq == body.seq);
    let standalone;
    let event_ref = match selected_event {
        Some(e) => e,
        None => {
            // Event not in a causal tree (no correlation_id) — load directly
            match scout_run::get_event_by_seq(&pool, body.seq).await {
                Ok(row) => {
                    standalone = row;
                    match &standalone {
                        Some(e) => e,
                        None => {
                            return (StatusCode::NOT_FOUND, "Event not found").into_response();
                        }
                    }
                }
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to load event: {e}")).into_response();
                }
            }
        }
    };

    let context = build_context(event_ref, &tree_rows);

    // Build Claude agent with investigation tools
    let mut claude = Claude::new(api_key, "claude-sonnet-4-20250514")
        .tool(LoadCausalTreeTool { pool: pool.clone() })
        .tool(SearchEventsTool { pool: pool.clone() })
        .tool(GetEventTool { pool: pool.clone() })
        .tool(GetSignalTool { reader: state.reader.clone() })
        .tool(FindEventsForNodeTool { pool: pool.clone() })
        .tool(GetRunInfoTool { pool });

    if let (Some(token), Some(repo)) = (&state.config.github_token, &state.config.github_repo) {
        claude = claude.tool(CreateGitHubIssueTool {
            github_token: token.clone(),
            github_repo: repo.clone(),
        });
    }

    // Build message history, prepending event context to the first user message
    let mut messages: Vec<Message> = Vec::new();
    let mut context_prepended = false;
    for msg in &body.messages {
        match msg.role.as_str() {
            "user" => {
                if !context_prepended {
                    messages.push(Message::user(format!("{}\n\n{}", context, msg.content)));
                    context_prepended = true;
                } else {
                    messages.push(Message::user(&msg.content));
                }
            }
            "assistant" => messages.push(Message::assistant(&msg.content)),
            _ => {}
        }
    }

    tracing::info!(
        message_count = messages.len(),
        event_seq = body.seq,
        tree_size = tree_rows.len(),
        "Starting agentic investigation"
    );

    // Use send() with multi_turn for agentic tool use
    let result = claude
        .prompt("")
        .preamble(SYSTEM_PROMPT)
        .messages(messages)
        .temperature(0.3)
        .multi_turn(15)
        .send()
        .await;

    match result {
        Ok(text) => {
            // Return as a single SSE frame to keep frontend SSE parsing working
            let sse_stream = futures::stream::once(async move {
                Ok::<_, Infallible>(Event::default().data(text))
            });
            Sse::new(sse_stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Investigation failed");
            let sse_stream = futures::stream::once(async move {
                Ok::<_, Infallible>(Event::default().event("error").data(e.to_string()))
            });
            Sse::new(sse_stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
    }
}
