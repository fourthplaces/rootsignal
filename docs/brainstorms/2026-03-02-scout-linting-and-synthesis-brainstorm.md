---
date: 2026-03-02
topic: scout-linting-and-synthesis
---

# Scout Linting & Signal Synthesis

## Context

Exploring whether scout's architecture lets LLMs be LLMs. Started from the idea of "two AIs talking to each other" (searcher + critic), refined into two distinct capabilities the system needs.

## Key Insight: Capture vs Sense-Making

The pipeline serves two fundamentally different jobs with opposite requirements:

- **Capture**: Faithful snapshot of reality. Mechanical, consistent, schema-adherent. The current extraction pipeline is well-suited for this — we *want* it to be a structured parser, not a creative reasoner.
- **Sense-making**: What does this snapshot mean? Requires curiosity, holistic awareness, pattern recognition. This is where LLMs are underutilized today.

These two jobs should stay separate. The LLM plays a different role in each.

## Two Planned Capabilities

### 1. Signal Linting (new)

A quality gate after extraction + mechanical dedup, before signal creation.

- **Batch-aware**: Reviews 10-20 signals at once with situational context
- **Semantic quality**: Catches things mechanical scoring can't — spam that looks legit, duplicate concepts with different titles, clickbait disguised as community resources
- **Re-investigation loop**: Can emit `ReinvestigationRequested` events to trigger targeted re-scrapes within the same phase (max 2 iterations)
- **Doesn't modify signals**: Triages (keep/reject/reinvestigate), doesn't alter the faithful capture
- **Supplements mechanical scoring**: The existing 4-layer dedup and field-completeness checks stay; the linter handles the semantic layer

Pipeline position:
```
Extract → Mechanical Dedup → Linter Batch Review → Signal Creation
                                    ↓ (reinvestigate)
                         targeted re-scrape → re-extract → back to Linter
```

### 2. Signal Synthesis & Connection (exists, needs deepening)

The synthesis phase already has 6 parallel roles (similarity, response mapping, tension linker, response finder, gathering finder, investigation). But each role is narrow and task-scoped — no LLM ever gets a holistic view of what was discovered in a run.

Potential improvements (not yet designed):
- A synthesis step that sees the full batch of created signals and reasons about connections
- Situation weaving that's more contextually aware of the emerging narrative
- Investigation that can follow curiosity rather than being assigned specific targets

## What Stays the Same

- Event-sourced phase progression (DAG stays clean)
- Seesaw engine and handler pattern
- Mechanical extraction (LLM as structured parser)
- 4-layer dedup
- All existing synthesis finders (additive, not replacement)

## Open Questions

- What context does the linter need? Active situations, regional knowledge, or both?
- How aggressive should the linter be? Need calibration strategy.
- Should synthesis deepening be a separate brainstorm or part of the linting work?

## Long-Term: Fine-Tuned Model on RootSignal's Dataset

Strategic goal: train a domain-specific model on the data the system is already accumulating. Three outcomes, in order of feasibility:

### 1. Fine-tuned extraction model (most feasible)

Train a smaller model (8B-13B) on (page content → structured signals) pairs. The event store already contains the output side; the archive contains the input side.

- Needs ~5K+ page→signal pairs with quality labels
- Quality labels come for free: signals that survived dedup, got corroborated, or were assigned to situations are positive examples
- Result: a cheap, fast model that matches or exceeds Haiku for *this specific domain*
- Doesn't need to generalize — only needs to understand community signals with our schema and tag vocabulary

### 2. Fine-tuned quality/linting model (medium feasibility)

Train on signal→quality label pairs. Becomes the linter from the section above, but running on a self-hosted model instead of vendor LLM.

- Needs ~2K+ labeled examples (keep/reject/reinvestigate decisions)
- The linting system, once built, generates this training data naturally
- Bootstrap with vendor LLM linting, then distill into a fine-tuned model over time

### 3. Self-hosted synthesis (hardest, lowest priority)

Synthesis tasks (tension linking, situation weaving) involve complex multi-signal reasoning with less training data. Vendor LLM likely stays here longest.

### Target end state

```
Fine-tuned model (cheap, fast, domain-specific) → bulk extraction + linting
Vendor LLM (expensive, smart, general-purpose)   → synthesis + investigation only
```

Each model used for what it's best at. Cost reduction on the high-volume path, quality preservation on the low-volume reasoning path.

### What to do now to keep this door open

The current architecture is well-positioned — event sourcing means we're building a dataset whether we planned to or not. Three low-cost additions to maximize future training value:

1. **Confirm raw source content preservation.** The archive module likely stores full page content sent to the LLM. Verify that (page text, extraction result) pairs are recoverable. Without the input side, we lose half of each training pair.

2. **Log negative examples.** Dedup logs rejections, but junk filters and future linting decisions should also persist rejected signals with rejection reasons. Negative examples are as valuable as positive ones for training.

3. **Human feedback signal.** If/when the supervisor or users can flag "this signal is wrong" or "this is great," those labels are worth 10x automated quality scores. Even a simple thumbs-up/down on signals in the admin app would compound over time.

None of these require architectural changes — just logging discipline.

## Next Steps

- Signal linting is the most concrete, implementable idea — plan that first
- Linting naturally produces training data for future fine-tuned quality model
- Synthesis deepening needs more brainstorming before planning
- Fine-tuning is a long-term bet; focus now on making sure the right data is being captured
