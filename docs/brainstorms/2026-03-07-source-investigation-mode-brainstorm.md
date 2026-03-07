---
date: 2026-03-07
topic: source-investigation-mode
---

# Single-Source Investigation Mode

## What We're Building

A fifth investigation mode (`"source"`) for deep-diving into one scout source. The operator clicks "Investigate" on `/scout/sources/:id` and can ask any question — productivity, diagnostics, network role, signal duplication, quality, history — in one conversation with tools tailored to that source's full context.

## Why This Approach

The existing `"sources"` mode is batch triage ("which of these 20 are garbage?"). This is a deep dive on ONE source — different question, different context, different tools. It follows the same pattern as the other four modes: context builder + tailored tools + system prompt.

## Key Decisions

- **Front-load core profile as context:** Source metadata (weight, penalty, scrape stats, discovery method, gap context, archive summary) goes into initial context. Heavier queries are tool-callable on demand.
- **Duplicate detection via signals list:** No special tool needed — `get_signals_produced` returns all signals; the AI spots duplicates within the same source by inspecting titles/summaries across scrapes.
- **Three new tools, rest reused:** Only `get_signals_produced`, `get_discovery_tree`, and `get_source_history` are new. Existing tools (`search_events`, `fetch_url`, `get_findings_for_node`, `get_run_info`, `get_source_info`) carry over.

## Tool Set

| Tool | New? | Purpose |
|------|------|---------|
| `get_signals_produced` | Yes | All signals from this source (type, title, summary, confidence, date) |
| `get_discovery_tree` | Yes | Ancestors + descendants with productivity stats |
| `get_source_history` | Yes | Timeline of key events for this source (creation, first signal, weight/penalty changes, deactivation) |
| `search_events` | No | Find events mentioning this source |
| `fetch_url` | No | Peek at current source content |
| `get_findings_for_node` | No | Supervisor quality flags |
| `get_run_info` | No | Scout run metadata |
| `get_source_info` | No | Detailed source metrics |

## Implementation Shape

### Backend (`investigate.rs`)
- New `"source"` mode in request dispatch
- Context builder: load SourceNode by ID, format core profile as initial context
- System prompt: explain source metrics, what "healthy" looks like, common patterns (duplicates, declining productivity, discovery value vs direct yield)
- Tool registration: 3 new + 5 existing

### Backend (`investigate_tools.rs`)
- `GetSignalsProducedTool` — query Neo4j for signals with this source's canonical_key
- `GetDiscoveryTreeTool` — walk ancestors + descendants in Neo4j with productivity stats
- `GetSourceHistoryTool` — query events for this source's lifecycle events (created, scraped, signals produced, weight/penalty changes)

### Frontend (`SourceDetailPage.tsx`)
- "Investigate" button in header (next to Scout and View Events)
- Opens `InvestigateDrawer` with `mode: "source"`, `sourceId: source.id`

## Open Questions

- None — scope is clear.

## Next Steps

-> `/workflows:plan` for implementation details
