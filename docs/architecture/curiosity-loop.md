# Curiosity Loop: Why Does This Signal Exist?

The Curiosity Loop is the scout's investigative engine. It takes signals — events, gives, asks, notices — and asks *"why does this exist?"* to discover the underlying **tensions** that caused them.

## Problem

The extraction pipeline produces high-quality signals, but signals are surface-level. A "Know Your Rights Workshop" is a Give, but *why* does it exist? The answer — ICE enforcement fear — is the tension that makes the signal meaningful. Without tensions, the graph is a flat list of resources. With tensions, it becomes a causal map of community life.

The curiosity loop bridges signals to tensions the same way a journalist bridges events to stories.

## Design Philosophy

### Not everything is curious

A pub trivia night is just a pub trivia night. The LLM's first job is to decide whether a signal warrants investigation. The `curious: bool` field in the extraction output lets the system skip self-explanatory signals without burning the agentic investigation budget.

This is different from the Response Scout, which doesn't need a worthiness check because its inputs are *already* tensions — identified problems that almost always have discoverable responses.

### Follow threads, not templates

The LLM has `web_search` and `read_page` tools. It reasons about what it finds:

```
Signal: "Know Your Rights Workshop — free legal workshop for immigrants"
  → curious: yes
  → search: "immigration enforcement Minneapolis 2026"
  → read article about ICE workplace raids
  → search: "ICE raids impact Minneapolis community"
  → read about community fear, school attendance drops
  → extract tension: "ICE enforcement fear causing community withdrawal"
```

Up to 8 tool turns allows 2-3 hops of investigation depth.

### Landscape-aware to avoid rediscovery

The LLM receives all existing tensions as context before investigating. If a signal points to "ICE enforcement fear" and that tension already exists, the LLM matches rather than re-creates. This focuses investigation energy on discovering tensions *not yet in the landscape*.

### Dedup at the embedding layer

Even when the LLM creates a "new" tension, `find_duplicate(embedding, NodeType::Tension, 0.85)` catches semantic matches. "ICE workplace raids causing fear" and "Immigration enforcement anxiety" would be deduplicated via cosine similarity, with the existing tension receiving a `RESPONDS_TO` edge from the signal.

## Architecture

### Two-phase pattern

1. **Agentic investigation** — multi-turn conversation with `web_search` + `read_page` tools. The LLM explores freely, following promising threads up to 8 tool turns.

2. **Structured extraction** — single `extract()` call to get a structured `SignalFinding` JSON: curious/not-curious, skip reason, and up to 3 discovered tensions with severity, category, evidence URL, and match strength.

### Target selection

```cypher
MATCH (n)
WHERE (n:Give OR n:Event OR n:Ask OR n:Notice)
  AND (n.curiosity_investigated IS NULL OR n.curiosity_investigated = 'failed')
  AND NOT (n)-[:RESPONDS_TO]->(:Tension)
  AND n.confidence >= 0.5
ORDER BY n.extracted_at DESC
LIMIT 10
```

Key decisions:
- **Excludes signals already wired to tensions** — `NOT (n)-[:RESPONDS_TO]->(:Tension)` prevents re-investigating signals that already have tension context.
- **Retries failures** — signals that failed investigation reappear until they succeed or hit the 3-retry cap (`abandoned`).
- **Confidence threshold at 0.5** — filters out low-quality signals that would produce unreliable tensions.
- **Most recent first** — `extracted_at DESC` prioritizes fresh signals where tensions are most actionable.
- **10 targets per run** — double the response scout's 5, because curiosity's pre-check (`curious: bool`) means many targets skip the expensive agentic phase.

### Investigation lifecycle

```
NULL (never investigated)
  ↓ investigate
"done" (curious=true, tensions found) — permanent
"skipped" (curious=false) — permanent
"failed" (error or LLM failure)
  ↓ retry (up to 3x)
"abandoned" (3 consecutive failures) — permanent
```

The pre-pass in `find_curiosity_targets` promotes exhausted retries:
```cypher
MATCH (n)
WHERE n.curiosity_investigated = 'failed'
  AND n.curiosity_retry_count >= 3
SET n.curiosity_investigated = 'abandoned'
```

### Finding processing

For each `DiscoveredTension`:
1. **Embed** title+summary via TextEmbedder
2. **Dedup** via `find_duplicate(embedding, Tension, 0.85)` — match or create
3. **Create tension** (if new) at **confidence 0.7** with city-center geo
4. **Wire `RESPONDS_TO` edge** from the original signal to the tension

### Budget

Per signal investigated:
- 1 Claude Haiku call for the agentic phase (multi-turn with tools)
- ~3 web searches (via `web_search` tool)
- ~2 Chrome page reads (via `read_page` tool)
- 1 Claude Haiku call for structured extraction

Per signal skipped (not curious):
- 1 Claude Haiku call for the agentic phase (returns quickly with no tool use)
- 1 Claude Haiku call for structured extraction

The cheap pre-check means ~30% of targets skip the expensive tool phase, saving significant budget.

## Key Design Rationale

### Why two phases instead of one?

The agentic phase produces free-form reasoning — rich, contextual, but unstructured. The extraction phase converts this into typed data. Combining them would constrain the LLM's investigation (forcing it to think in JSON while searching) or produce unreliable structured output (JSON generated mid-reasoning).

The response scout uses this same two-phase pattern for the same reason.

### Why confidence 0.7 for discovered tensions?

High enough to immediately qualify as a Response Scout target (threshold: 0.5). A tension discovered through multi-hop web investigation with evidence URLs is substantially more credible than an emergent tension discovered as a side-effect (which gets 0.4). The 0.7 confidence means the curiosity→response chain completes in a single scout run.

### Why max 3 tensions per signal?

Most signals relate to 1-2 tensions. Allowing 3 accommodates signals at intersections (a "immigrant food shelf" relates to both food insecurity and immigration enforcement fear). More than 3 would produce low-confidence, speculative tensions.

### Why provide the tension landscape as context?

Without it, the LLM would re-discover "housing affordability crisis" from every housing-related signal. The landscape prevents redundant investigation and focuses the LLM on *novel* tensions — the highest-value discoveries.

### Why retry failed investigations?

Failures are often transient — a Serper rate limit, a page that was temporarily down, a Chrome timeout. Three retries with the pre-pass promotion to `abandoned` ensures legitimate failures don't clog the target queue indefinitely.

### Why not check worthiness before investigating (like response scout doesn't)?

The curiosity loop *does* check worthiness — but it's integrated into the investigation itself. The `curious: bool` field means the LLM evaluates worthiness as part of the first phase, not as a separate call. A dedicated pre-filter would add a Haiku call to every target for a check that the investigation already performs for free.

## Files

| File | Role |
|------|------|
| `modules/rootsignal-scout/src/tension_linker.rs` | Main module: struct, prompts, tool wrappers (`WebSearchTool`, `ReadPageTool`), investigation, finding processing |
| `modules/rootsignal-graph/src/writer.rs` | Target selection: `find_curiosity_targets`, `mark_curiosity_investigated`, `get_tension_landscape` |
| `modules/rootsignal-scout/src/budget.rs` | Budget constants: `CLAUDE_HAIKU_CURIOSITY`, `SEARCH_CURIOSITY`, `CHROME_CURIOSITY` |
| `modules/rootsignal-scout/src/scout.rs` | Integration: runs curiosity loop before response scout |
