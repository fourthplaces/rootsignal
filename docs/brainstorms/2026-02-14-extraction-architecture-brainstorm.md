---
date: 2026-02-14
topic: extraction-architecture
---

# Extraction Architecture with AI

## Problem Statement

The current extraction pipeline uses a **single GPT-4o structured output call** per page snapshot to extract ~30 fields into an `ExtractedListing`. This produces two quality issues:

1. **Missing fields** — the AI returns nulls for data clearly present on the page (times, locations, contact info). The massive schema and long content create attention dilution.
2. **Hallucinated data** — the AI fabricates dates, organizations, or addresses not present in the source. No verification step catches this before normalization.

The existing pipeline: `page_snapshot → single AI call → extraction → normalize → listing`

## Reference Architecture: mntogether Curator

The mntogether project has a battle-tested multi-pass curator pipeline that solves these exact problems. Its key insight: **separate analysis from synthesis from safety**.

### mntogether's Pattern

```
1. BRIEF EXTRACTION (map — parallel, memoized LLM calls)
   - Per-page extraction of focused fields
   - Memoized by content hash (30-day TTL)
   - Max 10 concurrent calls

2. ORG DOCUMENT (deterministic — no LLM)
   - Compiles briefs + existing data into a single document
   - Budget-aware (~200k chars / ~50k tokens)
   - Prioritizes: website > social > existing posts

3. CURATOR REASONING (reduce — single focused LLM call)
   - Reads compiled document holistically
   - Proposes actions: create, update, merge, archive, flag
   - Includes reasoning and confidence per action

4. WRITER PASS (parallel — Claude Sonnet)
   - Polishes copy for human-quality output
   - Sees existing feed to avoid angle duplication

5. SAFETY REVIEW (iterative — up to 3 fix attempts)
   - Checks each action against its source briefs
   - Auto-fixes where possible, blocks if unfixable
   - Focuses on eligibility restrictions, misleading claims

6. STAGING (deterministic)
   - Creates proposals for admin review
   - Deduplicates by title similarity
```

### Key Architectural Decisions in mntogether

| Decision | Rationale |
|----------|-----------|
| Map-reduce over single-shot | Parallel briefing + focused reasoning = better accuracy |
| Memoization on briefs | Same page content doesn't need re-extraction |
| Deterministic document compilation | No LLM needed for assembly — saves cost, adds determinism |
| Separate writer from extractor | Different models excel at different tasks |
| Iterative safety loop | One fix attempt often isn't enough; 3 attempts with re-review |
| Proposals, not direct writes | Human-in-the-loop before data goes live |

---

## Proposed Approaches for Root Signal

### Approach A: Multi-Pass Analysis Pipeline (Recommended)

Adapt mntogether's proven pattern to Root Signal's extraction context. The core idea: **analysis → summary → safety check against analysis**.

#### Pipeline Stages

```
PAGE SNAPSHOT (markdown/html content)
    ↓
STAGE 1: CONTENT ANALYSIS (per-snapshot, memoizable)
    Model: Claude Sonnet (or Haiku for cost savings)
    Task: Extract structured brief from page content
    Output: ContentAnalysis {
        listings_detected: Vec<ListingBrief>,
        source_language: String,
        page_context: String,  // what kind of page is this
    }

    Each ListingBrief contains:
    - title, description, organization
    - raw_timing_text (exact quotes from page)
    - raw_location_text (exact quotes from page)
    - raw_contact_text (exact quotes from page)
    - categories, audience signals
    - relevance_score
    ↓
STAGE 2: STRUCTURED EXTRACTION (per-listing)
    Model: Claude Sonnet
    Task: Convert brief into full ExtractedListing schema
    Input: ListingBrief + taxonomy instructions
    Output: ExtractedListing (existing schema)

    Key difference from today:
    - Operates on a focused brief, not the full 30k page
    - Has raw text quotes to ground against
    - Taxonomy mapping is isolated from content extraction
    ↓
STAGE 3: SAFETY / GROUNDING CHECK (per-listing)
    Model: Claude Haiku (fast, cheap)
    Task: Compare extracted listing against original ContentAnalysis
    Input: ExtractedListing + original ContentAnalysis + source text
    Output: GroundingResult {
        verdict: "safe" | "fix" | "blocked",
        field_issues: Vec<FieldIssue>,
        // e.g., "start_time not found in source text"
        fixes: Option<PartialExtractedListing>,
    }

    Rules:
    - Flag any field whose value can't be traced to source text
    - Flag dates that don't appear in raw_timing_text
    - Flag addresses that don't appear in raw_location_text
    - Auto-fix where possible (null out ungrounded fields)
    - Block listings that are fundamentally fabricated
    ↓
STAGE 4: NORMALIZE (existing code, mostly unchanged)
    Same normalization into entities/listings/tags
    But now with per-field confidence from grounding check
    ↓
STAGE 5: EMBED (existing code, unchanged)
```

#### What Changes vs. Today

| Component | Current | Proposed |
|-----------|---------|----------|
| AI calls per snapshot | 1 (GPT-4o) | 1 analysis + N extractions + N safety checks |
| Model | GPT-4o only | Claude Sonnet (analysis/extraction) + Haiku (safety) |
| Content sent to extraction | Full 30k page | Focused brief (~500-1000 chars) |
| Hallucination detection | None | Grounding check with field-level verdicts |
| Missing field mitigation | None | Raw text quotes in analysis → better extraction |
| Memoization | None | Analysis stage can be memoized by content hash |
| Field-level confidence | Single confidence_hint | Per-field grounded/ungrounded from safety check |

#### Restate Workflow Changes

The `ExtractWorkflow` would change from:

```
for snapshot in snapshots:
    extraction_ids = extract_from_snapshot(snapshot)  // 1 AI call
    for extraction in extractions:
        normalize(extraction)
```

To:

```
for snapshot in snapshots:
    analysis = analyze_snapshot(snapshot)              // AI call 1
    for brief in analysis.listings_detected:
        extraction = extract_from_brief(brief)        // AI call 2
        grounding = check_grounding(extraction, analysis)  // AI call 3
        if grounding.verdict != "blocked":
            apply_fixes(extraction, grounding)
            normalize(extraction)
```

#### Cost Estimate

Assuming ~3 listings per page on average:
- **Today**: 1 GPT-4o call (~$0.01-0.03 per snapshot)
- **Proposed**: 1 Sonnet + 3 Sonnet + 3 Haiku (~$0.02-0.06 per snapshot)
- ~2x cost increase for significantly better quality

---

### Approach B: Extract + Verify (Quick Win)

Minimal change — keep single-shot extraction but add a grounding verification pass.

```
PAGE SNAPSHOT
    ↓
EXTRACT (switch from GPT-4o to Claude Sonnet, same prompt)
    ↓
VERIFY (new — Claude Haiku)
    Compare each field against source text
    Null out ungrounded fields
    Flag suspicious extractions
    ↓
NORMALIZE (unchanged)
```

**Pros:**
- Minimal code changes — add one activity, modify workflow
- Catches hallucinations effectively
- Easy to ship incrementally

**Cons:**
- Doesn't fix missing fields (same attention dilution problem)
- Still sending 30k of content to a single extraction call
- No raw text grounding — verifier has to re-read the full page

**Estimated effort:** 1-2 days. Add `verify_extraction` activity + modify workflow.

---

### Approach C: Citation-Required Extraction

Single extraction call, but change the output schema to require citations.

```rust
struct CitedField<T> {
    value: T,
    citation: Option<String>,  // exact quote from source
    confidence: f32,
}

struct CitedExtractedListing {
    title: CitedField<String>,
    description: CitedField<Option<String>>,
    start_time: CitedField<Option<String>>,
    // ... all fields wrapped in CitedField
}
```

**Pros:**
- One API call — no orchestration change
- Built-in grounding — no citation = low confidence
- Easy to audit downstream

**Cons:**
- ~2x output tokens (citations are verbose)
- Doesn't help with missing fields
- Taxonomy fields (listing_type, categories) don't have natural citations
- Requires new schema, breaking change to `ExtractedListing`

**Estimated effort:** 2-3 days. New schema, prompt changes, normalization updates.

---

### Approach D: Decomposed Schema Extraction

Break the single 30-field extraction into focused domain-specific calls:

```
PAGE SNAPSHOT
    ↓ (parallel)
    ├── CORE EXTRACTOR: title, description, org, listing_type, source_url
    ├── TIMING EXTRACTOR: start_time, end_time, recurring, schedule
    ├── LOCATION EXTRACTOR: address, city, state, postal_code, location_text
    ├── CONTACT EXTRACTOR: name, email, phone
    └── TAXONOMY MAPPER: categories, audience_roles, signal_domain, urgency, etc.
    ↓
MERGE + VERIFY
    ↓
NORMALIZE
```

**Pros:**
- Each extractor has a focused, small schema — much less hallucination
- Parallel execution — potentially faster than sequential multi-pass
- Easy to tune individual extractors independently
- Missing fields improve because each extractor focuses on one domain

**Cons:**
- 5+ API calls per snapshot (though they can run in parallel)
- Merge step adds complexity — what if two extractors disagree?
- Harder to share context between extractors

**Estimated effort:** 3-5 days. Five new activities, merge logic, workflow changes.

---

## Recommendation

**Start with Approach A (Multi-Pass Analysis Pipeline)** because:

1. It follows the proven mntogether pattern that already works in production
2. The "analysis → summary → safety check against analysis" flow directly addresses both missing fields AND hallucination
3. Raw text quotes in the analysis stage give the extraction stage grounding material
4. The safety check is a natural audit gate before normalization
5. Memoization on the analysis stage saves money on re-extractions

**Incremental path:**
1. Ship Approach B first (extract + verify) as a quick win — 1-2 days
2. Then evolve to Approach A by adding the analysis stage — 3-5 more days
3. Approach B's verify step becomes Approach A's safety check

This lets you get immediate quality improvements while building toward the full architecture.

## Key Decisions Still Needed

- **Memoization strategy**: Cache analysis by content hash in DB? TTL?
- **Blocked listing handling**: Silently drop, or surface for human review?
- **Confidence thresholds**: What per-field confidence triggers nulling vs. keeping?
- **Model selection**: Claude Sonnet for all passes, or Haiku for safety check?
- **Batch vs. per-listing safety**: Check all listings from a page together, or individually?

## Open Questions

- Should we add a writer pass (like mntogether) to polish listing descriptions?
- Do we need a proposal/staging layer, or is direct-to-listing okay for Root Signal?
- How should we handle the existing `ExtractedListing` schema — evolve it or create new types?
- Should the analysis stage detect page type (events page vs. services page vs. blog) to select different extraction strategies?

## Next Steps

-> `/workflows:plan` for implementation details on the chosen approach
