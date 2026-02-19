# Response Scout Architecture

The Response Scout is the inverse of the curiosity loop. Where curiosity asks *"why does this signal exist?"* and discovers **tensions**, the Response Scout asks *"what diffuses this tension?"* and discovers **responses** — Gives, Events, and Asks that address community problems.

## Problem

45 tensions exist in the graph but most sit at `heat=0` with no `RESPONDS_TO` edges. The existing response mapping only matches signals *already in the graph*. It cannot find responses that haven't been scraped yet. The Response Scout finds these missing responses through creative, multi-hop LLM investigation.

## Design Philosophy

### Follow threads, don't fill templates

The LLM reasons about mechanisms, not categories:

```
Tension: "ICE enforcement fear"
  → search: "how is ICE enforcement funded Minneapolis"
  → read article about ICE contracts with private companies
  → infer: economic pressure is a lever
  → search: "boycott ICE contractors Minneapolis"
  → find: actual boycott campaign page
  → evaluate: does this DIFFUSE the tension? (yes — removes funding)
  → extract as response signal
```

### Diffusing vs escalating

The system amplifies responses that *diffuse* tension — non-compliance, economic pressure, sanctuary, mutual aid, legal leverage, information campaigns. NOT responses that escalate — retaliation, counter-violence, divisive framing.

Examples of diffusion mechanisms (not exhaustive — the LLM can invent new ones):
- **Non-compliance**: removes the system's power
- **Economic pressure**: removes funding/oxygen
- **Sanctuary**: creates zones the tension can't reach
- **Mutual aid**: makes communities resilient enough to weather the tension
- **Legal leverage**: uses the system's own rules against it
- **Information**: dissolves fear through knowledge
- **Creative action**: art, protest, culture that transforms the narrative

### Give signals as heuristics

Existing Gives that respond to a tension hint at what response categories exist. "Know your rights workshop" → legal education is a response type → search for more of the same, plus types not yet represented.

### Emergent by design

The LLM may discover:
- A response that diffuses **multiple tensions** (wire to all of them)
- A **new tension** nobody anticipated (create it, like curiosity does)
- A novel diffusion mechanism that doesn't fit any pre-defined category
- Connections between tensions that weren't previously visible

The output schema accommodates all of these. The prompt encourages them.

## Architecture

### Two-phase pattern (mirrors curiosity loop)

1. **Agentic investigation** — multi-turn conversation with `web_search` + `read_page` tools. Up to 10 tool turns (deeper than curiosity's 8) to allow following threads 2-3 hops deep.

2. **Structured extraction** — single `extract()` call to get structured `ResponseFinding` JSON containing discovered responses, emergent tensions, and future query seeds.

### Target selection

```cypher
MATCH (t:Tension)
WHERE t.confidence >= 0.5
  AND coalesce(datetime(t.response_scouted_at), datetime('2000-01-01'))
      < datetime() - duration('P14D')
OPTIONAL MATCH (t)<-[:RESPONDS_TO]-(r)
WITH t, count(r) AS response_count
ORDER BY response_count ASC, t.cause_heat DESC, t.confidence DESC
LIMIT $limit
```

Key decisions:
- **Not limited to zero-response tensions** — under-served tensions also qualify. Sorting by `response_count ASC` means the most neglected tensions get attention first.
- **Timestamp instead of boolean** — `response_scouted_at` allows periodic re-scouting (14-day window). Tensions evolve; new responses emerge.
- **Confidence threshold at 0.5** — filters out low-confidence/emergent tensions that need corroboration first.

### Finding processing

For each `DiscoveredResponse`:
1. **Embed** title+summary via TextEmbedder
2. **Dedup** via `writer.find_duplicate(embedding, node_type, 0.85)`
   - If match: reuse existing signal, still create RESPONDS_TO edge if missing
   - If new: create signal node directly (same pattern as curiosity creates tensions)
3. **Create node** — Give/Event/Ask with city-center geo, 0.7 confidence
4. **Wire RESPONDS_TO edge** to the target tension
5. **Wire `also_addresses`** — embed each freeform tension title, cosine-search against all active tension embeddings (>0.85 threshold). ~45 tensions makes in-memory cosine trivial.

For each `EmergentTension`:
- Create TensionNode with **confidence capped at 0.4** — below the 0.5 target selection threshold, so emergent tensions require external corroboration before becoming Response Scout targets. Prevents infinite loops.

For `future_queries`:
- Create TavilyQuery sources with `SourceRole::Response` to seed next-run discovery.

### Integration point

Runs in `Scout::run_inner()` after the curiosity loop and before story weaving:

```
Clustering → Response Mapping → Curiosity Loop → Response Scout → Story Weaving → Investigation
```

### Budget

Per tension investigated:
- 3 Claude Haiku calls (investigation + structuring)
- 5 Tavily searches
- 3 Chrome page reads

The scout checks budget availability before starting and skips if exhausted.

## Key Design Rationale

### Why direct node creation (not re-scraping)?

The curiosity loop proves this pattern works. The LLM already has full context about how the response relates to the tension. Re-scraping through the generic extractor would add latency, might misclassify, and lose tension context.

### Why freeform `diffusion_mechanism`?

An enum kills emergence. The LLM might discover "digital sanctuary," "labor organizing," "diaspora networks," or mechanisms nobody anticipated. Freeform strings let the system learn new categories organically.

### Why allow emergent tension discovery?

The investigation might reveal "boycott organizers facing retaliation" — a new tension embedded in the response landscape. Capturing these makes the system antifragile: investigating responses surfaces new problems, which surface new responses, in a virtuous cycle.

### Why `also_addresses`?

A mutual aid network might diffuse both "ICE fear" and "housing affordability." Wiring one response to multiple tensions reflects reality and avoids redundant investigation.

### Why embedding similarity for `also_addresses` (not string matching)?

"Fear of ICE raids" and "ICE enforcement anxiety" would fail string similarity but embed nearly identically. Vector search is more robust.

### Why cap emergent tension confidence at 0.4?

Prevents an infinite loop: Curiosity creates tensions (0.7 confidence) → Response Scout finds responses + emergent tensions → emergent tensions trigger more Response Scout runs. At 0.4, emergent tensions are BELOW the 0.5 threshold in target selection, so they genuinely require external corroboration before becoming Response Scout targets.

### Why enforce URL provenance?

LLMs hallucinate URLs, especially during multi-hop reasoning. Requiring that `url` must be a page the agent actually visited via `read_page` ensures every response is backed by a real, verified source. The prompt constraint makes this an extraction rule rather than a post-hoc filter.

### Why verify event dates?

Events are inherently temporal. A "Know Your Rights workshop" from 2023 injected as a current response would mislead users. The prompt instructs the LLM to only extract current/future events, and the optional `event_date` field allows the frontend to age out stale events gracefully.

## Files

| File | Role |
|------|------|
| `modules/rootsignal-scout/src/response_scout.rs` | Main module: struct, prompts, investigation, finding processing |
| `modules/rootsignal-scout/src/curiosity.rs` | Tool types shared via `pub(crate)`: `SearcherHandle`, `ScraperHandle`, `WebSearchTool`, `ReadPageTool` |
| `modules/rootsignal-graph/src/writer.rs` | New methods: `find_response_scout_targets`, `get_existing_responses`, `mark_response_scouted` |
| `modules/rootsignal-scout/src/budget.rs` | Budget constants: `CLAUDE_HAIKU_RESPONSE_SCOUT`, `TAVILY_RESPONSE_SCOUT`, `CHROME_RESPONSE_SCOUT` |
| `modules/rootsignal-scout/src/scout.rs` | Integration: runs response scout after curiosity loop |

## Future Work

**Tension as Gravity.** A separate investigation mode: "where are people gathering around this tension?" — solidarity events, vigils, singing resistance, luminaries. Distinct from instrumental responses. Deserves its own design session.
