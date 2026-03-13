---
title: "feat: Rich Annotated Briefs with Inline Citations"
type: feat
date: 2026-03-13
---

# Rich Annotated Briefs with Inline Citations

## Overview

Situation briefs gain inline `[signal:UUID]` citation annotations. The LLM embeds signal references directly in the briefing markdown. The frontend parses these into superscript citation numbers with click-to-open popovers showing signal title, source URL, and summary. A top-level synthesis indicator shows how many signals and sources informed the brief.

V1 scope: `[signal:UUID]` citations only. Actor, location, and external URL annotations deferred to V2 (the LLM doesn't receive those UUIDs today).

## Problem Statement

Briefs are narrative summaries that synthesize information from multiple signals, but readers have no way to trace claims back to their sources. Dispatches already use `[signal:UUID]` citations, but briefs — the primary reader-facing content — do not. This makes briefs unverifiable and prevents readers from drilling into the underlying evidence.

## Proposed Solution

Extend the existing dispatch citation pattern to briefs:

1. **Backend**: Pass signal UUIDs into the briefing LLM prompt (they're available on `WeaveSignal` but not currently included). Instruct the LLM to embed `[signal:UUID]` tokens inline. Validate citations post-generation.
2. **Frontend**: Build a remark plugin that transforms `[signal:UUID]` tokens into `<CitationRef>` components. Render as superscript numbers with popovers. Use the existing `signals` field from the `SITUATION_DETAIL` query as the lookup map — no new GraphQL queries needed.

## Technical Approach

### Phase 1: Backend — Annotated Briefing Generation

#### 1a. Enrich the briefing prompt with signal UUIDs

**File**: `modules/rootsignal-scout/src/domains/cluster_weaving/activities.rs:35-57`

Current `build_first_weave_prompt` formats signals as:
```
- [Concern] Title: Summary...
```

Change to include UUIDs and source URLs:
```
- [signal:UUID] (Concern) Title — source: https://... — Summary...
```

The LLM needs three things per signal: the UUID to cite, the type for context, and the source URL so it can reference original sources.

`WeaveSignal` already has `id`, `node_type`, `title`, `summary`, `url` — no new queries needed.

#### 1b. Add citation instructions to SYSTEM_PROMPT

**File**: `modules/rootsignal-scout/src/domains/cluster_weaving/activities.rs:29-33`

Append to the cluster weaving `SYSTEM_PROMPT`:

```
When referencing information from a specific signal, cite it inline using [signal:UUID] format.
Cite key factual claims — names, numbers, dates, quoted statements, specific events.
Do not cite general narrative sentences or transitions.
Every signal ID you cite MUST come from the provided signal list. Never invent IDs.
```

Keep the instructions lighter than the dispatch system prompt (13 hard rules). Briefs are narratives, not forensic reports — over-citation would harm readability.

#### 1c. Add citation validation

**File**: `modules/rootsignal-scout/src/domains/cluster_weaving/activities.rs` (in `first_weave` and `reweave`)

After LLM generation, before emitting the event:

1. Parse `[signal:UUID]` tokens from `briefing_body` using a generalized version of `extract_signal_citations()` from `situation_weaving/activities/pure.rs:236`
2. Build a set of valid signal UUIDs from the `signals` vec passed to the prompt
3. Strip any citations referencing UUIDs not in the valid set (LLM hallucination)
4. Log a warning if any citations were stripped

No retry on invalid citations. A brief with fewer citations is better than burning LLM budget.

#### 1d. Generalize the citation parser

**File**: `modules/rootsignal-common/src/lib.rs` (or a new `annotations.rs`)

Move `extract_signal_citations` from `situation_weaving/activities/pure.rs` to a shared location. Generalize to parse any `[type:identifier]` annotation:

```rust
pub struct Annotation {
    pub kind: String,      // "signal", "actor", "location", "url"
    pub identifier: String, // UUID string or URL
    pub raw: String,        // "[signal:abc-123]" — for stripping/replacing
}

pub fn extract_annotations(body: &str) -> Vec<Annotation> { ... }
pub fn extract_signal_ids(body: &str) -> Vec<Uuid> { ... } // convenience wrapper
pub fn strip_invalid_citations(body: &str, valid_ids: &HashSet<Uuid>) -> String { ... }
```

This serves V1 (signals only) and is ready for V2 (actors, locations, URLs).

### Phase 2: Frontend — Citation Rendering (search-app)

#### 2a. Build a remark plugin for annotation parsing

**File**: `modules/search-app/src/lib/remarkAnnotations.ts` (new)

A remark plugin that:
1. Walks the markdown AST text nodes
2. Finds `[signal:UUID]` patterns via regex
3. Replaces them with custom MDAST nodes (e.g., `{ type: 'citation', signalId: '...' }`)

This runs in the remark pipeline before ReactMarkdown renders.

#### 2b. Build the CitationRef component

**File**: `modules/search-app/src/components/CitationRef.tsx` (new)

A component that:
- Renders as a superscript number: `<sup className="cursor-pointer text-blue-400">[1]</sup>`
- On click, opens a popover (using Floating UI or a simple absolute-positioned div)
- Popover shows: signal title, signal type badge, summary (truncated), source URL link
- For stacked citations (`[signal:A][signal:B]`), renders as `1,2` with a combined popover listing both signals

Citation numbering: academic style — same signal UUID = same number throughout the brief. Numbers assigned in order of first appearance.

#### 2c. Build the CitationContext provider

**File**: `modules/search-app/src/components/CitationContext.tsx` (new)

A React context that:
1. Takes the `signals` array from the SITUATION_DETAIL query
2. Builds a `Map<string, Signal>` lookup by ID
3. Assigns citation numbers (first-appearance order based on parsing the briefing body)
4. Provides `getSignal(id)`, `getCitationNumber(id)`, `totalCitations`, `totalSources`

This decouples data resolution from the remark plugin and CitationRef component.

#### 2d. Add the synthesis indicator

**File**: `modules/search-app/src/components/SituationDetail.tsx:218-223`

Above the briefing body, render:
```
Synthesized from N signals across M sources
```

Where:
- N = count of distinct signal UUIDs cited in the brief
- M = count of distinct source URLs among cited signals

Use `CitationContext` to compute these counts.

#### 2e. Wire it all together in SituationDetail

**File**: `modules/search-app/src/components/SituationDetail.tsx`

```tsx
<CitationProvider signals={signals} briefingBody={situation.briefingBody}>
  <SynthesisIndicator />
  <ReactMarkdown
    remarkPlugins={[remarkGfm, remarkAnnotations]}
    components={{
      ...briefingComponents,
      citation: CitationRef, // custom component for annotation nodes
    }}
  >
    {situation.briefingBody}
  </ReactMarkdown>
</CitationProvider>
```

#### 2f. Graceful degradation

- If `briefingBody` contains no `[signal:UUID]` tokens (old briefs), render as plain markdown — no change
- If a cited signal UUID is not found in the `signals` array (expired/purged signal), show the citation number but the popover says "Source no longer available"
- If `briefingBody` is null, fall back to lede (existing behavior)

### Phase 3: Dispatch Citation Rendering (bonus)

Dispatch bodies already contain `[signal:UUID]` tokens but render as raw text. The same remark plugin and CitationRef component work here with no additional backend changes.

**File**: `modules/search-app/src/components/SituationDetail.tsx:295-297`

Replace the plain text dispatch body:
```tsx
<p className="text-muted-foreground whitespace-pre-wrap">{dispatch.body}</p>
```

With:
```tsx
<CitationProvider signals={signals} briefingBody={dispatch.body}>
  <ReactMarkdown remarkPlugins={[remarkGfm, remarkAnnotations]} components={{...}}>
    {dispatch.body}
  </ReactMarkdown>
</CitationProvider>
```

This is free — the infrastructure built for briefs works identically for dispatches.

## Acceptance Criteria

### Functional

- [ ] `build_first_weave_prompt` includes signal UUIDs and source URLs in the signal list
- [ ] Cluster weaving `SYSTEM_PROMPT` instructs the LLM to cite signals inline
- [ ] LLM-generated `briefing_body` contains `[signal:UUID]` tokens for key claims
- [ ] Invalid citations (hallucinated UUIDs) are stripped before the event is emitted
- [ ] Warning logged when citations are stripped
- [ ] search-app renders `[signal:UUID]` as superscript citation numbers
- [ ] Clicking a citation number opens a popover with signal title, type, summary, and source link
- [ ] Stacked citations (`[signal:A][signal:B]`) render as grouped numbers with combined popover
- [ ] Same signal cited multiple times uses the same citation number (academic style)
- [ ] Synthesis indicator shows "from N signals across M sources" above the brief
- [ ] Old briefs without annotations render unchanged (graceful degradation)
- [ ] Missing signals in citation lookup show "Source no longer available" in popover
- [ ] Dispatch bodies also render with citation popovers (Phase 3)

### Non-Functional

- [ ] Generic `extract_annotations` parser lives in `rootsignal-common`, not in a specific domain
- [ ] Remark plugin is a separate module, reusable across search-app and admin-app
- [ ] No new GraphQL queries — citation data resolved from existing `signals` field

## Dependencies & Risks

**Dependencies:**
- `WeaveSignal` struct already has `id`, `url` — no graph query changes needed
- `SITUATION_DETAIL` query already fetches `signals(limit: 50)` — no schema changes needed
- Existing `extract_signal_citations` in `pure.rs` provides the parsing pattern

**Risks:**
- **LLM citation quality**: The LLM may hallucinate UUIDs or under-cite. Mitigation: strip invalid citations, don't retry. Monitor citation density over time.
- **Token budget**: Adding UUIDs to the prompt increases input tokens. 50 signals × ~120 chars each = ~6K chars extra. Manageable.
- **Markdown parser conflicts**: `[signal:UUID]` could conflict with markdown link syntax. Mitigation: remark plugin runs before link parsing, and the `signal:` prefix is unambiguous.
- **Citation density**: Over-cited briefs hurt readability. Mitigation: prompt instructs "cite key factual claims, not narrative transitions."

## References & Research

### Internal References

- Brainstorm: `docs/brainstorms/2026-03-13-rich-annotated-briefs-brainstorm.md`
- Briefing generation: `modules/rootsignal-scout/src/domains/cluster_weaving/activities.rs:35-57`
- Dispatch citation system: `modules/rootsignal-scout/src/domains/situation_weaving/activities/pure.rs:236-298`
- Dispatch verification: `modules/rootsignal-scout/src/domains/situation_weaving/activities/weave.rs:239`
- WeaveSignal struct: `modules/rootsignal-graph/src/writer.rs:3589-3600`
- Frontend rendering: `modules/search-app/src/components/SituationDetail.tsx:218-223`
- GraphQL query: `modules/search-app/src/graphql/queries.ts:345-403`

### Prior Art

- **Perplexity Sonar API**: inline `[n]` markers + `citations` URL array — closest production pattern
- **Pandoc citation syntax**: `[@key]` — established academic standard
- **rehype-citation**: remark/rehype plugin for Pandoc-style citations with tooltips
- **littlefoot.js**: footnote-to-popover conversion library — gold standard for the popover UX
- **Anthropic Citations API**: interleaved content blocks with cited_text — richer but brief is no longer a string
