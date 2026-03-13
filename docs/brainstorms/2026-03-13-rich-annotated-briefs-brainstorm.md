---
date: 2026-03-13
topic: rich-annotated-briefs
---

# Rich Annotated Briefs

## What We're Building

Situation briefs become annotated markdown with inline structured references. The LLM embeds `[type:identifier]` tokens for every entity it mentions that has a destination (signals, actors, locations, external URLs). Each consuming platform parses these annotations and renders them as appropriate — the brief format stays platform-agnostic.

The search-app renders annotations as superscript citation numbers with click-to-open popovers showing source context. A top-level synthesis indicator ("from N signals across M sources") gives readers a quick credibility read.

## Why This Approach

Three architectures exist in production:

- **Inline tokens** (Perplexity, Pandoc): markers like `[1]` in text + separate source lookup. Simple, text stays valid markdown.
- **Offset sidecar** (OpenAI, Gemini): clean text + parallel `{start, end, source}` array. Powerful but offsets break on any text transformation.
- **Interleaved blocks** (Anthropic Citations API, Notion): text segmented into blocks with attached citations. Rich but the brief is no longer a single string.

We chose **inline tokens** because:
- The brief stays a plain markdown string — works on any platform
- `[signal:UUID]` already exists in dispatches, so we're extending a proven convention
- No offset fragility
- The remark/rehype ecosystem has plugins for this pattern
- Any consumer can parse `[type:identifier]` with regex

## Key Decisions

- **Annotation format**: `[type:identifier]` — extensible to any entity type. Known types: `[signal:UUID]`, `[actor:UUID]`, `[location:UUID]`, `[url:https://...]`
- **Multiple citations stack**: `[signal:UUID1][signal:UUID2]` on the same claim
- **Citation UX (search-app)**: superscript numbers, popover on click showing title + source + extracted quote + link to entity page
- **Per-claim counts**: `[3]` means 3 signals back this claim, popover lists all three
- **Top-level indicator**: "Synthesized from N signals across M sources" header
- **Platform-agnostic storage**: annotated markdown stored as-is in `briefing_body` — no schema change, just richer content
- **Backend reuses dispatch citation infra**: same verification (cited IDs exist in graph), same `[signal:UUID]` convention
- **LLM gets signal UUIDs + metadata** so it can reference them inline
- **Every linkable entity gets annotated**: signals, actors, locations, Instagram profiles, news articles — anything with a page to go to

## Open Questions

- Should the LLM also output a structured `citations` JSON array alongside the markdown (like Perplexity's sidecar), or is parsing `[type:id]` tokens from the markdown sufficient?
- How do we handle annotation rendering on platforms that don't support popovers (e.g., plain markdown export, email)? Graceful degradation to footnotes or stripped text?
- Should external URLs (Instagram profiles, news articles) be stored as graph entities with UUIDs, or referenced directly by URL in annotations?
- Citation verification: fail the brief and retry, or emit with warnings and flag for review?

## Next Steps

→ `/workflows:plan` for implementation details
