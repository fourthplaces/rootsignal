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
use uuid::Uuid;

use futures::future::join_all;

use ai_client::{Agent, Claude, Message, PromptBuilder};
use rootsignal_common::SourceNode;

use crate::db::scout_run::{self, event_layer, event_summary, json_str};
use crate::investigate_tools::{
    CreateGitHubIssueTool, DeactivateSourcesTool, FetchUrlTool, FindEventsForNodeTool,
    GetEventTool, GetFindingsForNodeTool, GetRunInfoTool, GetSignalTool, GetSourceInfoTool,
    LoadCausalTreeTool, SearchEventsTool,
};
use crate::jwt;
use crate::AppState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "mode")]
pub enum InvestigateRequest {
    #[serde(rename = "event")]
    Event {
        seq: i64,
        messages: Vec<ChatMessage>,
    },
    #[serde(rename = "sources")]
    Sources {
        source_ids: Vec<Uuid>,
        messages: Vec<ChatMessage>,
    },
    #[serde(rename = "scout_run")]
    ScoutRun {
        run_id: String,
        messages: Vec<ChatMessage>,
    },
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    role: String,
    content: String,
}

// ---------------------------------------------------------------------------
// Event mode — system prompt + context builder
// ---------------------------------------------------------------------------

const EVENT_SYSTEM_PROMPT: &str = r#"You are an event investigation assistant for RootSignal, an event-sourced community intelligence platform. Your job is to help operators understand what happened and why by analyzing events and their causal chains.

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
- `fetch_url` — fetch a source page to compare what it actually says vs what was extracted
- `get_findings_for_node` — check if the supervisor already flagged quality issues for a signal or source
- `get_source_info` — look up source metadata (weight, quality penalty, cadence, discovery method, production stats)

## How to Respond

Talk like you're explaining what happened to a colleague. Be conversational and direct — tell the story of what happened, why, and what it triggered. Mention seq numbers naturally when they help ("that kicked off event 4231, which...") but don't build tables or numbered checklists unless the user asks for them.

Keep it concise. If something is interesting or unusual, say so. If the user wants a step-by-step timeline or structured breakdown, they'll ask in a follow-up.

If you spot something that looks like a bug or warrants a ticket, say so and offer to file a GitHub issue. Only call the `create_github_issue` tool after the user explicitly confirms — never create issues unprompted.
"#;

/// Cap the selected event payload so the context fits within token budgets.
const MAX_PAYLOAD_CHARS: usize = 10_000;

fn truncate_payload(json: &str) -> String {
    if json.len() <= MAX_PAYLOAD_CHARS {
        json.to_string()
    } else {
        format!(
            "{}…\n\n(payload truncated — use get_event tool to see the full payload)",
            &json[..json[..MAX_PAYLOAD_CHARS]
                .rfind('\n')
                .unwrap_or(MAX_PAYLOAD_CHARS)]
        )
    }
}

fn build_event_context(
    event: &scout_run::EventRowFull,
    tree: &[scout_run::EventRowFull],
) -> String {
    let name = json_str(&event.data, "type").unwrap_or_else(|| event.event_type.clone());
    let summary = event_summary(&name, &event.data);
    let layer = event_layer(&event.event_type);
    let payload = serde_json::to_string_pretty(&event.data).unwrap_or_default();
    let payload = truncate_payload(&payload);

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

// ---------------------------------------------------------------------------
// Sources mode — system prompt + context builder
// ---------------------------------------------------------------------------

const SOURCES_SYSTEM_PROMPT: &str = r#"You are a source quality auditor for RootSignal, a community intelligence platform that scrapes web sources to extract signals about local communities.

## What You're Looking At

You've been given a selection of sources that an operator wants to evaluate. The context includes a table with key stats for each source.

## Key Metrics Explained

- **weight** (0.0–1.0): Operator-assigned importance. Higher = scraped more often and results prioritized. Default 0.5.
- **quality_penalty** (0.0–1.0): System-assigned penalty for low-quality output. Default 1.0 (no penalty). Lower = worse quality history.
- **effective_weight**: `weight × quality_penalty` — the actual priority used for scheduling. Low effective weight means the source gets scraped less.
- **signals_produced**: Total signals (gatherings, resources, concerns, etc.) ever extracted from this source.
- **scrape_count**: Total number of times this source has been scraped.
- **consecutive_empty_runs**: How many recent scrapes in a row produced zero signals. High numbers suggest the source has gone stale or was never productive.
- **sources_discovered**: How many child sources were found via link promotion from this source. A source that discovers other sources has value even with low direct signal production.
- **discovery_method**: How the source was found — curated (manually added), link_promotion (discovered from another source's content), web_query (found via search), human_submission (user-submitted).

## Deactivation Criteria

A source is likely unproductive if:
- It has many consecutive empty runs (5+ is a red flag)
- It has zero or very few signals produced relative to its scrape count
- It has discovered zero child sources
- Its quality penalty is very low (system already flagged it)

But be cautious about:
- **Curated** or **human_submission** sources — these were deliberately added and may have strategic value
- Sources that discover other sources — even with low direct signal production, they're valuable as seed sources
- Recently created sources that haven't had enough scrapes yet

## Your Tools

- `get_source_info` — look up detailed info for a single source by URL
- `fetch_url` — peek at what a source page actually contains right now
- `get_findings_for_node` — check if the supervisor already flagged quality issues
- `search_events` — find events mentioning a source
- `deactivate_sources` — deactivate sources by their UUIDs (only after user confirms)

## How to Respond

Be conversational and direct. When assessing sources:
1. Start with a quick overview of the selection — how many look healthy vs. unproductive
2. Call out the worst offenders specifically and explain why
3. Note any sources worth keeping despite poor numbers (e.g. seed sources, curated)
4. When asked to deactivate, list which sources and why, then ask for confirmation before calling the tool

Don't build giant tables unless asked. Talk like a colleague reviewing these together.
"#;

fn build_sources_context(sources: &[SourceNode]) -> String {
    let mut ctx = format!("## Selected Sources ({})\n\n", sources.len());
    ctx.push_str("| # | canonical_value | discovery | weight | penalty | eff_wt | signals | scrapes | empty_runs | sources_disc | active |\n");
    ctx.push_str("|---|----------------|-----------|--------|---------|--------|---------|---------|------------|--------------|--------|\n");

    for (i, s) in sources.iter().enumerate() {
        let eff = s.weight * s.quality_penalty;
        ctx.push_str(&format!(
            "| {} | {} | {:?} | {:.2} | {:.2} | {:.2} | {} | {} | {} | {} | {} |\n",
            i + 1,
            s.canonical_value,
            s.discovery_method,
            s.weight,
            s.quality_penalty,
            eff,
            s.signals_produced,
            s.scrape_count,
            s.consecutive_empty_runs,
            s.sources_discovered,
            if s.active { "yes" } else { "no" },
        ));
    }

    // Add UUIDs in a separate reference block so the AI can use them for tool calls
    ctx.push_str("\n### Source IDs\n\n");
    for s in sources {
        ctx.push_str(&format!("- `{}` — {}\n", s.id, s.canonical_value));
    }

    ctx
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

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

    let api_key = state.config.anthropic_api_key.clone();
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

    // Dispatch based on mode
    match body {
        InvestigateRequest::Event { seq, messages } => {
            handle_event_mode(state, pool, &api_key, seq, messages).await
        }
        InvestigateRequest::Sources {
            source_ids,
            messages,
        } => handle_sources_mode(state, pool, &api_key, source_ids, messages).await,
        InvestigateRequest::ScoutRun { run_id, messages } => {
            handle_run_mode(state, pool, &api_key, run_id, messages).await
        }
    }
}

async fn handle_event_mode(
    state: Arc<AppState>,
    pool: Arc<sqlx::PgPool>,
    api_key: &str,
    seq: i64,
    chat_messages: Vec<ChatMessage>,
) -> axum::response::Response {
    // Load the selected event and its causal tree from the database
    let (tree_rows, _root_seq) = match scout_run::causal_tree(&pool, seq).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, seq, "Failed to load causal tree");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load event: {e}"),
            )
                .into_response();
        }
    };

    // Find the selected event in the tree, or load it separately
    let selected_event = tree_rows.iter().find(|r| r.seq == seq);
    let standalone;
    let event_ref = match selected_event {
        Some(e) => e,
        None => {
            match scout_run::get_event_by_seq(&pool, seq).await {
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
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load event: {e}"),
                    )
                        .into_response();
                }
            }
        }
    };

    let context = build_event_context(event_ref, &tree_rows);

    // Build Claude agent with event investigation tools
    let mut claude = Claude::new(api_key, "claude-sonnet-4-20250514")
        .tool(LoadCausalTreeTool { pool: pool.clone() })
        .tool(SearchEventsTool { pool: pool.clone() })
        .tool(GetEventTool { pool: pool.clone() })
        .tool(GetSignalTool {
            reader: state.reader.clone(),
        })
        .tool(FindEventsForNodeTool { pool: pool.clone() })
        .tool(GetRunInfoTool { pool })
        .tool(FetchUrlTool)
        .tool(GetFindingsForNodeTool {
            reader: state.reader.clone(),
        })
        .tool(GetSourceInfoTool {
            writer: state.writer.clone(),
        });

    if let (Some(token), Some(repo)) = (&state.config.github_token, &state.config.github_repo) {
        claude = claude.tool(CreateGitHubIssueTool {
            github_token: token.clone(),
            github_repo: repo.clone(),
        });
    }

    tracing::info!(
        message_count = chat_messages.len(),
        event_seq = seq,
        tree_size = tree_rows.len(),
        "Starting event investigation"
    );

    run_agent(claude, EVENT_SYSTEM_PROMPT, &context, &chat_messages).await
}

async fn handle_sources_mode(
    state: Arc<AppState>,
    pool: Arc<sqlx::PgPool>,
    api_key: &str,
    source_ids: Vec<Uuid>,
    chat_messages: Vec<ChatMessage>,
) -> axum::response::Response {
    // Load sources from the graph
    let sources = match state.writer.get_sources_by_ids(&source_ids).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to load sources for investigation");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load sources: {e}"),
            )
                .into_response();
        }
    };

    if sources.is_empty() {
        return (StatusCode::NOT_FOUND, "No matching sources found").into_response();
    }

    let context = build_sources_context(&sources);

    // Build Claude agent with source audit tools
    let claude = Claude::new(api_key, "claude-sonnet-4-20250514")
        .tool(GetSourceInfoTool {
            writer: state.writer.clone(),
        })
        .tool(FetchUrlTool)
        .tool(GetFindingsForNodeTool {
            reader: state.reader.clone(),
        })
        .tool(SearchEventsTool { pool })
        .tool(DeactivateSourcesTool {
            writer: state.writer.clone(),
        });

    tracing::info!(
        message_count = chat_messages.len(),
        source_count = sources.len(),
        "Starting source audit investigation"
    );

    run_agent(claude, SOURCES_SYSTEM_PROMPT, &context, &chat_messages).await
}

// ---------------------------------------------------------------------------
// Scout run mode — system prompt + context builder
// ---------------------------------------------------------------------------

const SCOUT_RUN_SYSTEM_PROMPT: &str = r#"You are a scout run investigation assistant for RootSignal, an event-sourced community intelligence platform. Your job is to help operators understand what a scout run did, why it made the decisions it made, and whether anything looks off.

## What is a Scout Run?

A scout run is a single execution of the scouting pipeline for a region. It scrapes web sources, extracts signals (gatherings, resources, concerns, etc.), deduplicates them against existing data, discovers new sources, and collects expansion queries for future runs.

## Run Context

You've been given:
- **Run metadata**: region, timing, flow type, stats
- **Event breakdown**: counts of key event types so you can see the shape of the run
- **Sample events**: a selection of notable events (failures, rejections, discovered sources, signals created) so you have concrete data to discuss

## Key Stats Explained

- **urls_scraped**: How many source URLs were fetched
- **signals_stored**: How many new signals made it into the graph
- **signals_deduplicated**: How many signals were detected as duplicates of existing data
- **expansion_sources_created**: How many new sources were discovered during this run
- **expansion_queries_collected**: Search queries generated for future source discovery
- **handler_failures**: Pipeline handler errors — non-zero means something broke

## Your Tools

You have the full investigation toolkit. The run's metadata, stats, event breakdown, and sample events are already loaded in context — no need to re-fetch them. Use tools to drill deeper:
- `search_events` — find events by keyword (URLs, signal names, error messages)
- `get_event` — load full payload of a specific event by seq number
- `load_causal_tree` — trace the causal chain from any event
- `get_signal` — inspect a signal node in the graph
- `find_events_for_node` — find all events that touched a signal or source
- `get_run_info` — compare against other runs (this run's info is already in context)
- `get_source_info` — look up source metadata (weight, quality, production stats)
- `fetch_url` — fetch a source page to see what it actually contains
- `get_findings_for_node` — check if the supervisor flagged quality issues

## How to Respond

Talk like you're debriefing a colleague. Start with the big picture — was this a healthy run or a troubled one? Then highlight anything interesting:
- Sources that produced nothing or failed
- Signals that got rejected and why
- New sources discovered and whether they look promising
- Unusual patterns (e.g. high dedup rate, many failures, zero signals from a source that usually produces)

Be conversational and direct. Mention seq numbers when they help trace specifics. Don't build tables unless asked — tell the story.

If you spot something that looks like a bug, say so and offer to file a GitHub issue. Only call `create_github_issue` after the user explicitly confirms.
"#;

fn build_run_context(
    run: &scout_run::ScoutRunRow,
    variant_counts: &[(& str, i64)],
    sample_events: &[(& str, Vec<scout_run::EventRow>)],
) -> String {
    let duration = run.finished_at.map(|f| {
        let secs = (f - run.started_at).num_seconds();
        if secs < 60 {
            format!("{secs}s")
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    });

    let mut ctx = format!(
        "## Scout Run\n\n\
         - **run_id**: {}\n\
         - **region**: {}\n\
         - **flow_type**: {}\n\
         - **started_at**: {}\n\
         - **finished_at**: {}\n\
         - **duration**: {}\n",
        run.run_id,
        run.region,
        run.flow_type.as_deref().unwrap_or("default"),
        run.started_at,
        run.finished_at
            .map(|t| t.to_string())
            .unwrap_or_else(|| "still running".to_string()),
        duration.unwrap_or_else(|| "running".to_string()),
    );

    // Stats
    ctx.push_str("\n### Stats\n\n");
    ctx.push_str(&format!(
        "| Metric | Value |\n|--------|-------|\n\
         | URLs scraped | {} |\n\
         | Signals extracted | {} |\n\
         | Signals stored | {} |\n\
         | Signals deduplicated | {} |\n\
         | Expansion sources created | {} |\n\
         | Expansion queries collected | {} |\n\
         | Handler failures | {} |\n",
        run.stats.urls_scraped.unwrap_or(0),
        run.stats.signals_extracted.unwrap_or(0),
        run.stats.signals_stored.unwrap_or(0),
        run.stats.signals_deduplicated.unwrap_or(0),
        run.stats.expansion_sources_created.unwrap_or(0),
        run.stats.expansion_queries_collected.unwrap_or(0),
        run.stats.handler_failures.unwrap_or(0),
    ));

    // Input sources if present
    if let Some(ref source_ids) = run.source_ids {
        if let Some(arr) = source_ids.as_array() {
            if !arr.is_empty() {
                ctx.push_str("\n### Input Sources\n\n");
                for id in arr {
                    if let Some(s) = id.as_str() {
                        ctx.push_str(&format!("- `{s}`\n"));
                    }
                }
            }
        }
    }

    // Event type breakdown
    if !variant_counts.is_empty() {
        ctx.push_str("\n### Event Breakdown\n\n");
        ctx.push_str("| Event Type | Count |\n|------------|-------|\n");
        for (variant, count) in variant_counts {
            ctx.push_str(&format!("| {variant} | {count} |\n"));
        }
    }

    // Sample events
    for (label, events) in sample_events {
        if events.is_empty() {
            continue;
        }
        ctx.push_str(&format!("\n### Sample: {label}\n\n"));
        ctx.push_str("| seq | name | summary |\n|-----|------|----------|\n");
        for e in events {
            let name =
                json_str(&e.data, "type").unwrap_or_else(|| e.event_type.clone());
            let summary = event_summary(&name, &e.data);
            ctx.push_str(&format!(
                "| {} | {} | {} |\n",
                e.seq,
                name,
                summary.as_deref().unwrap_or("-"),
            ));
        }
    }

    ctx
}

/// Variants we count for the event breakdown table.
const BREAKDOWN_VARIANTS: &[&str] = &[
    "content_fetched",
    "content_unchanged",
    "content_fetch_failed",
    "signals_extracted",
    "new_signal_accepted",
    "observation_rejected",
    "cross_source_match_detected",
    "same_source_reencountered",
    "source_discovered",
    "sources_discovered",
    "source_registered",
    "source_rejected",
    "expansion_query_collected",
    "handler_failed",
    "handler_skipped",
];

/// Variants we sample (with small limits) to give the agent concrete data.
const SAMPLE_VARIANTS: &[(&str, &str, i64)] = &[
    ("Failures", "handler_failed", 10),
    ("Fetch Failures", "content_fetch_failed", 10),
    ("Rejections", "observation_rejected", 15),
    ("Signals Created", "new_signal_accepted", 15),
    ("Sources Discovered (old)", "source_discovered", 15),
    ("Sources Proposed", "sources_discovered", 10),
    ("Sources Registered", "source_registered", 15),
    ("Sources Rejected", "source_rejected", 10),
    ("Dedup Matches", "cross_source_match_detected", 10),
];

async fn handle_run_mode(
    state: Arc<AppState>,
    pool: Arc<sqlx::PgPool>,
    api_key: &str,
    run_id: String,
    chat_messages: Vec<ChatMessage>,
) -> axum::response::Response {
    // Load run metadata
    let run = match scout_run::find_by_id(&pool, &run_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Run not found").into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, run_id, "Failed to load scout run");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load run: {e}"),
            )
                .into_response();
        }
    };

    // Count key event types (all queries in parallel)
    let count_futures = BREAKDOWN_VARIANTS.iter().map(|variant| {
        let pool = pool.clone();
        let run_id = run_id.clone();
        async move {
            let count = scout_run::count_events_by_variant(&pool, &run_id, variant)
                .await
                .unwrap_or(0);
            (*variant, count)
        }
    });
    let variant_counts: Vec<(&str, i64)> = join_all(count_futures)
        .await
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect();

    // Sample notable events (all queries in parallel)
    let sample_futures = SAMPLE_VARIANTS.iter().map(|(label, variant, limit)| {
        let pool = pool.clone();
        let run_id = run_id.clone();
        async move {
            let rows = scout_run::list_events_by_variant(&pool, &run_id, variant, *limit)
                .await
                .unwrap_or_default();
            (*label, rows)
        }
    });
    let sample_events: Vec<(&str, Vec<scout_run::EventRow>)> = join_all(sample_futures)
        .await
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .collect();

    let context = build_run_context(&run, &variant_counts, &sample_events);

    // Build Claude agent with the full investigation toolkit
    let mut claude = Claude::new(api_key, "claude-sonnet-4-20250514")
        .tool(LoadCausalTreeTool { pool: pool.clone() })
        .tool(SearchEventsTool { pool: pool.clone() })
        .tool(GetEventTool { pool: pool.clone() })
        .tool(GetSignalTool {
            reader: state.reader.clone(),
        })
        .tool(FindEventsForNodeTool { pool: pool.clone() })
        .tool(GetRunInfoTool { pool })
        .tool(FetchUrlTool)
        .tool(GetFindingsForNodeTool {
            reader: state.reader.clone(),
        })
        .tool(GetSourceInfoTool {
            writer: state.writer.clone(),
        });

    if let (Some(token), Some(repo)) = (&state.config.github_token, &state.config.github_repo) {
        claude = claude.tool(CreateGitHubIssueTool {
            github_token: token.clone(),
            github_repo: repo.clone(),
        });
    }

    tracing::info!(
        message_count = chat_messages.len(),
        run_id = %run_id,
        variant_counts = variant_counts.len(),
        sample_sections = sample_events.len(),
        "Starting scout run investigation"
    );

    run_agent(claude, SCOUT_RUN_SYSTEM_PROMPT, &context, &chat_messages).await
}

async fn run_agent(
    claude: Claude,
    system_prompt: &str,
    context: &str,
    chat_messages: &[ChatMessage],
) -> axum::response::Response {
    // Build message history, prepending context to the first user message
    let mut messages: Vec<Message> = Vec::new();
    let mut context_prepended = false;
    for msg in chat_messages {
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

    let result = claude
        .prompt("")
        .preamble(system_prompt)
        .messages(messages)
        .temperature(0.3)
        .multi_turn(15)
        .send()
        .await;

    match result {
        Ok(text) => {
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
