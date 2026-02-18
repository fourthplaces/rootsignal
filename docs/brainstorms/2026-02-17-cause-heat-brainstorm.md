---
date: 2026-02-17
topic: cause-heat
---

# Cause Heat: Cross-Story Signal Boosting

## What We're Building
A "cause heat" score for every signal that measures how much independent community attention exists in its semantic neighborhood. Signals are boosted not by their own posting frequency, but by the community's collective attention to the cause they serve.

## Why This Approach
A small food shelf that posted once a week ago should rise in the system when the housing crisis is trending — because poverty connects housing and food. The boost comes from the cause, not from posting. We use embedding similarity (already computed) to discover these connections automatically, with no taxonomy or LLM categorization needed.

## Key Decisions
- **No explicit "Cause" nodes or taxonomy**: Cause relationships are emergent from embedding similarity between signals
- **Source diversity drives heat, not raw count**: Self-promotion doesn't inflate cause heat (already fixed by source_diversity)
- **Batch computation in Rust**: Load all ~600 signal embeddings, compute pairwise similarity in memory (~360K dot products, <10ms), write back. No per-signal DB queries needed.
- **Continuous similarity, not categorical**: A food bank Ask gets partial credit from housing signals AND partial credit from volunteer signals, proportional to semantic distance
- **Formula**: `cause_heat(signal) = Σ cosine_sim(signal, neighbor) × neighbor.source_diversity` for neighbors above threshold

## Philosophy
You are rewarded simply for showing up for a good cause. A single post about food donations rises when many independent organizations are talking about food insecurity — even if that post is a week old. Post frequency doesn't matter. Only cause frequency / weight matters.

## Open Questions
- Cosine similarity threshold: 0.7? 0.75? Need to test against live embeddings.
- Normalization: raw sum, log scale, or 0-1 normalization?
- Should cause_heat feed into story energy, or be a separate ranking dimension?

## Next Steps
→ `/workflows:plan` for implementation details
