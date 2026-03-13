---
title: "feat: Add single-source investigation mode"
type: feat
date: 2026-03-07
---

# Single-Source Investigation Mode

## Overview

Add a fifth investigation mode (`"source_dive"`) that lets operators deep-dive into one scout source from `/scout/sources/:id`. Unlike the batch `"sources"` mode (which triages many sources), this mode answers any question about a single source — productivity, duplicate signals, diagnostics, discovery network role, quality trends.

## Proposed Solution

Follow the exact pattern of the four existing modes: request variant + context builder + system prompt + tool set + frontend `InvestigateMode` variant + `getModeConfig` case.

Three new investigation tools query Neo4j and Postgres for source-specific data. Existing general-purpose tools carry over.

## Acceptance Criteria

- [x] "Investigate" button on `SourceDetailPage` header opens `InvestigateDrawer` with `mode: "source_dive"`
- [x] Backend dispatches `mode: "source_dive"` to a new handler with source-specific context and tools
- [x] `get_signals_produced` tool returns signals from this source (capped at 50 via existing reader, includes total count)
- [x] `get_discovery_tree` tool returns ancestor chain + direct children with productivity stats
- [x] `get_source_history` tool returns key lifecycle events from Postgres, filtered to a curated event type list
- [x] Existing tools available: `search_events`, `fetch_url`, `get_findings_for_node`, `get_run_info`, `get_source_info` (note: looks up by URL, not UUID), `deactivate_sources` (with confirmation), `create_github_issue` (conditional on config)
- [x] System prompt explains source metrics, what "healthy" looks like, and common pathologies

## Implementation

### Phase 1: Backend — Request Dispatch + Context Builder

**`modules/rootsignal-api/src/investigate.rs`**

Add variant to `InvestigateRequest` enum (line 36-58):
```rust
#[serde(rename = "source_dive")]
SourceDive {
    source_id: Uuid,
    messages: Vec<ChatMessage>,
},
```

Add match arm in `investigate_handler` (line 305-321):
```rust
InvestigateRequest::SourceDive { source_id, messages } => {
    handle_source_dive_mode(state, pool, &api_key, source_id, messages).await
}
```

Add `build_source_dive_context(source: &SourceNode) -> String`:
- Format source profile as markdown table with fields: canonical_value, discovery_method, weight, quality_penalty, effective_weight (weight × penalty), signals_produced, scrape_count, consecutive_empty_runs, sources_discovered, active, cadence_hours, avg_signals_per_scrape, source_role, created_at, last_scraped, last_produced_signal
- Include gap_context in a separate section if present (analyst notes on why this source matters)
- Include source UUID so AI can use it in tool calls and deactivation

Add `handle_source_dive_mode()` following the pattern of `handle_sources_mode` (line 408-455):
- Load source via `writer.get_sources_by_ids(&[source_id])`, take first, 404 if empty with message "Source not found"
- Build context
- Configure Claude agent with `let mut claude` (must be mutable for conditional GitHub tool)
- Call `run_agent(claude, SOURCE_DIVE_SYSTEM_PROMPT, &context, &chat_messages)`

Add `SOURCE_DIVE_SYSTEM_PROMPT`:

```rust
const SOURCE_DIVE_SYSTEM_PROMPT: &str = r#"You are a source investigation assistant for RootSignal, a community intelligence platform that scrapes web sources to extract signals about local communities. Your job is to help operators deeply understand a single source — its productivity, quality, role in the discovery network, and anything worth acting on.

## What is a Source?

A source is a web or social input to the scouting pipeline — a URL, social media handle, or search query that gets scraped on a cadence to extract signals (gatherings, resources, concerns, help requests, announcements, conditions). Sources form a discovery network: one source can discover child sources via link promotion, creating a tree of related inputs.

## Source Metrics Explained

- **weight** (0.0–1.0): Operator-assigned importance. Higher = scraped more often. Default 0.5.
- **quality_penalty** (0.0–1.0): System-assigned penalty based on output quality. Default 1.0 (no penalty). Lower = system has flagged quality issues.
- **effective_weight**: `weight × quality_penalty` — actual scheduling priority.
- **signals_produced**: Total signals ever extracted from this source.
- **scrape_count**: Total scrapes, regardless of signal yield.
- **avg_signals_per_scrape**: Rolling average productivity.
- **consecutive_empty_runs**: Recent scrapes in a row with zero signals. 5+ is a red flag for staleness.
- **sources_discovered**: Child sources found via link promotion. A source with high discovery value is worth keeping even with low direct signal production.
- **discovery_method**: How the source was found — curated (manually added), link_promotion (discovered from another source), web_query (search), human_submission.
- **source_role**: What kind of signals this source tends to surface.
- **gap_context**: Analyst notes explaining why this source matters or what coverage gap it fills.

## What "Healthy" Looks Like

- Consistent signal production (avg_signals_per_scrape > 0, low consecutive_empty_runs)
- Reasonable effective_weight (not penalized into irrelevance)
- Active and producing recent signals (last_produced_signal within scrape cadence)

## Common Pathologies

- **Declining productivity**: Was productive, now many empty runs. Content may have changed or site structure broke.
- **Signal duplication**: Same signals appearing across multiple scrapes — check titles/summaries for repeats.
- **Discovery-only value**: Zero direct signals but discovered many child sources. Still valuable as a seed.
- **Quality penalty death spiral**: Low quality_penalty × decent weight = low effective_weight = rarely scraped = no chance to recover.
- **Stale curated source**: Manually added, never productive. Gap_context might explain why it was kept.
- **Orphaned branch**: Source is the root of a discovery tree where all descendants are also unproductive.

## Your Tools

The source profile is already loaded in context. Use tools to drill deeper:

- `get_signals_produced` — list all signals this source has produced (type, title, confidence, date). Use this to spot duplicates, assess quality, and understand what the source contributes.
- `get_discovery_tree` — see this source's ancestors (who discovered it) and descendants (what it discovered), with productivity stats for each node.
- `get_source_history` — timeline of lifecycle events for this source (scrapes, signal extractions, failures). Use to understand trends over time.
- `search_events` — find events mentioning this source by keyword. Good for tracing specific incidents.
- `find_events_for_node` — all events that touched this source by node_id.
- `get_event` — load full payload of a specific event by seq number.
- `get_run_info` — metadata about a scout run (stats, timing, region).
- `fetch_url` — peek at what the source page actually contains right now. Useful to compare current content against what was extracted.
- `get_source_info` — look up another source's metadata by URL substring (not UUID). Useful when comparing against related sources.
- `get_findings_for_node` — check if the supervisor already flagged quality issues.
- `deactivate_sources` — deactivate this source by UUID. Only call after the operator explicitly confirms.
- `create_github_issue` — file a bug if something looks broken. Only after operator confirms.

## How to Respond

The operator is already looking at this source's detail page — they can see the stats, signals table, and discovery tree. Don't recite numbers they already see. Instead, interpret what the data means and tell the story.

Talk like a colleague doing a deep review together. Be conversational and direct. When you spot something interesting — a pattern, an anomaly, a recommendation — say so plainly. Mention specifics (signal titles, event seq numbers, dates) when they help.

If you find the source is unproductive and should be deactivated, explain your reasoning and offer to deactivate — but only call the tool after the operator says yes.

If something looks like a bug (e.g., scraper failing silently, quality penalty applied incorrectly), say so and offer to file a GitHub issue.
"#;
```

### Phase 2: Backend — New Investigation Tools

**`modules/rootsignal-api/src/investigate_tools.rs`**

**Tool 1: `GetSignalsProducedTool`**
- Input: `source_id: String` (UUID)
- Calls `reader.signals_for_source(&uuid)` (already exists at `reader.rs:1289`)
- Returns: Vec of `{ id, title, signal_type, confidence, extracted_at, source_url }` — reader caps at 50 via existing `LIMIT 50` in Cypher
- Also runs a count query (`MATCH (n)-[:PRODUCED_BY]->(s:Source {id: $id}) RETURN count(n)`) and includes `total_count` in output so the AI knows when results are truncated ("showing 50 of 247 signals")
- Follows `GetSignalTool` struct pattern (takes `reader: Arc<PublicGraphReader>`)

**Tool 2: `GetDiscoveryTreeTool`**
- Input: `source_id: String` (UUID)
- Calls `reader.discovery_tree(&uuid)` (already exists at `reader.rs:1321`)
- Returns: nodes `{ id, canonical_value, discovery_method, active, signals_produced }` + edges `{ child_id, parent_id }` + `root_id`
- Follows `GetSignalTool` struct pattern (takes `reader: Arc<PublicGraphReader>`)

**Tool 3: `GetSourceHistoryTool`**
- Input: `canonical_key: String` (the source's canonical_value, provided by context)
- Queries Postgres `events` table using the same text search pattern as `SearchEventsTool`: `WHERE payload::text ILIKE '%' || $1 || '%'`
- Additionally filters to lifecycle-relevant event types via `AND payload->>'type' IN (...)`:

```rust
const SOURCE_LIFECYCLE_VARIANTS: &[&str] = &[
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
    "handler_failed",
];
```

- Returns: Vec of `{ seq, event_type, timestamp, summary }`, capped at 100, ordered chronologically (ASC, not DESC — timeline order)
- Follows `SearchEventsTool` struct pattern (takes `pool: Arc<PgPool>`)
- SQL:
```sql
SELECT seq, ts, event_type, payload AS data
FROM events
WHERE payload::text ILIKE '%' || $1 || '%'
  AND payload->>'type' = ANY($2)
ORDER BY seq ASC
LIMIT 100
```

Update imports in `investigate.rs` (line 22-26) to include the three new tools.

### Phase 3: Backend — Tool Wiring in handle_source_dive_mode

```rust
let mut claude = Claude::new(api_key, "claude-sonnet-4-20250514")
    .tool(GetSignalsProducedTool { reader: state.reader.clone() })
    .tool(GetDiscoveryTreeTool { reader: state.reader.clone() })
    .tool(GetSourceHistoryTool { pool: pool.clone() })
    .tool(SearchEventsTool { pool: pool.clone() })
    .tool(GetEventTool { pool: pool.clone() })
    .tool(FindEventsForNodeTool { pool: pool.clone() })
    .tool(GetRunInfoTool { pool: pool.clone() })
    .tool(FetchUrlTool)
    .tool(GetSourceInfoTool { writer: state.writer.clone() })
    .tool(GetFindingsForNodeTool { reader: state.reader.clone() })
    .tool(DeactivateSourcesTool { writer: state.writer.clone() });

if let (Some(token), Some(repo)) = (&state.config.github_token, &state.config.github_repo) {
    claude = claude.tool(CreateGitHubIssueTool {
        github_token: token.clone(),
        github_repo: repo.clone(),
    });
}
```

### Phase 4: Frontend — InvestigateDrawer Integration

**`modules/admin-app/src/components/InvestigateDrawer.tsx`**

Add variant to `InvestigateMode` type (line 57-61):
```typescript
| { mode: "source_dive"; sourceId: string; sourceLabel: string }
```

Add case to `getModeConfig` (line 81-126):
```typescript
case "source_dive":
  return {
    title: `Investigate: ${investigation.sourceLabel}`,
    subtitle: `source_id=${investigation.sourceId}`,
    autoMessage: "What's the story with this source? Anything unusual in its history, signal quality, or discovery network that's worth acting on?",
    loadingLabel: "Investigating source...",
    showSynthesis: false,
    buildBody: (messages) => ({ mode: "source_dive", source_id: investigation.sourceId, messages }),
  };
```

### Phase 5: Frontend — SourceDetailPage Button + Drawer

**`modules/admin-app/src/pages/SourceDetailPage.tsx`**

Add import:
```typescript
import { InvestigateDrawer, type InvestigateMode } from "@/components/InvestigateDrawer";
```

Add state:
```typescript
const [investigation, setInvestigation] = useState<InvestigateMode | null>(null);
```

Add "Investigate" button after "View Events" link (line 294), same styling:
```typescript
<button
  onClick={() => setInvestigation({
    mode: "source_dive",
    sourceId: source.id,
    sourceLabel: source.canonicalValue.length > 40
      ? source.canonicalValue.slice(0, 40) + "..."
      : source.canonicalValue,
  })}
  className="text-xs px-2.5 py-1 rounded-md border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors"
>
  Investigate
</button>
```

Add drawer overlay at the end of the component (before closing `</div>`), following the `SourcesPage` pattern (line 764-786):
```typescript
{investigation && (
  <div className="fixed inset-0 z-50 flex">
    <div className="flex-1 bg-black/40" onClick={() => setInvestigation(null)} />
    <div className="w-[520px] bg-card border-l border-border flex flex-col">
      <InvestigateDrawer
        key={investigation.sourceId}
        investigation={investigation}
        onClose={() => setInvestigation(null)}
      />
    </div>
  </div>
)}
```

## Technical Considerations

- **`"source_dive"` naming**: Chosen for lexical distance from existing `"sources"` mode. Avoids one-character-apart function names, system prompts, and context builders.
- **`let mut claude`**: Required for conditional `CreateGitHubIssueTool` addition. The existing `handle_sources_mode` uses immutable binding because it never adds the GitHub tool — this mode must follow `handle_event_mode`'s mutable pattern instead.
- **`signals_for_source` caps at 50**: The Cypher query at `reader.rs:1296` has `LIMIT 50` hardcoded. The tool adds a separate count query so the AI knows when results are truncated.
- **`get_source_info` takes URL, not UUID**: The system prompt explicitly notes this so the AI doesn't waste a tool call trying to look up by ID. The source's own data is already in context.
- **Token budget**: Source context is lean (one source profile). The heavier data (signals list, discovery tree, event history) lives behind tools, loaded on demand. Tools are capped: signals at 50, history at 100.
- **No new API route**: The existing `POST /api/investigate` with serde tag dispatch handles the new mode automatically.
- **No new GraphQL query**: `SOURCE_DETAIL` already provides the `id` needed to pass to the backend.
- **`showSynthesis: false`**: The synthesis prompt is event-specific (causal chains, seq numbers). A source-specific synthesis variant would be useful but is a separate feature.
- **Deactivation tool included**: Enables "investigate and act" in one flow — natural for an operator who discovers a source is garbage.

## References

- Brainstorm: `docs/brainstorms/2026-03-07-source-investigation-mode-brainstorm.md`
- Backend handler: `modules/rootsignal-api/src/investigate.rs:265-321`
- Backend tools: `modules/rootsignal-api/src/investigate_tools.rs`
- Frontend drawer: `modules/admin-app/src/components/InvestigateDrawer.tsx:57-126`
- Source detail page: `modules/admin-app/src/pages/SourceDetailPage.tsx:230-319`
- Graph reader (signals_for_source): `modules/rootsignal-graph/src/reader.rs:1289`
- Graph reader (discovery_tree): `modules/rootsignal-graph/src/reader.rs:1321`
- Event type constants: `modules/rootsignal-api/src/investigate.rs:608-639`
- Postgres event search: `modules/rootsignal-api/src/db/models/scout_run.rs:306-326`
