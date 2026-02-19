# Engagement-Aware Discovery

## The Problem

All tensions get equal discovery investment regardless of community engagement. A tension from one government PDF with zero corroboration gets the same discovery budget as one mentioned across 5 sources with social media buzz. The system spends equal effort seeking responses to both.

## Why Not a Gate

The system is designed to investigate first, prune later. Hard engagement gates would cause several problems:

**Bootstrap dead zone.** New cities start with zero engagement data. A gate would block all discovery until enough signals accumulate — a chicken-and-egg problem.

**Emergent chain suppression.** A novel tension with zero engagement today may be the seed of a critical story tomorrow. Gating kills tension→response→new-tension chains before they form.

**Matthew effect.** Popular tensions get more discovery, producing more signals, raising their engagement score further. Unpopular-but-important tensions get starved. This is the opposite of anti-fragility.

**False precision.** Engagement signals (especially `cause_heat`) depend on clustering, which runs asynchronously. A tension may have high real-world engagement but zero `cause_heat` because clustering hasn't run yet.

## The Approach: Budget Allocation, Not Blocking

Engagement data **ranks** tensions for discovery priority — it does not block any tension from receiving discovery queries.

### LLM-driven discovery

The briefing now includes engagement data for each tension:

```
1. [HIGH] "Food desert growing" — What would help: grocery co-op
   community attention: 3 sources, 2 corroborations, heat=0.7
```

The system prompt instructs the LLM to prioritize high-engagement tensions but always reserve at least 2 queries for low-engagement or novel tensions.

### Mechanical fallback

The mechanical fallback sorts tensions by engagement score before iterating, so high-engagement tensions fill early query slots. But the iteration processes all tensions (up to the existing MAX_GAP_QUERIES cap). No tension is skipped due to low engagement.

### Engagement score

The sort uses: `corroboration_count + source_diversity + cause_heat * 10.0`

The `* 10.0` on cause_heat normalizes it against the integer counts (cause_heat is 0.0–1.0, while corroboration and diversity are unbounded integers typically in the 0–10 range).

## Engagement Signals

| Signal | What it measures | Where computed | Timing |
|--------|-----------------|----------------|--------|
| `corroboration_count` | Number of independent signals that support this tension | `investigation.rs` during signal investigation | Available after first investigation cycle |
| `source_diversity` | Number of distinct sources that mention this tension | `investigation.rs` during signal investigation | Available after first investigation cycle |
| `cause_heat` | Clustering-derived measure of how active/discussed the tension's causal cluster is | `clustering.rs` during periodic clustering | Only available after clustering runs (may lag) |

## Design Principles

- **No hard gates.** Every tension gets discovery investment. Engagement ranks, never blocks.
- **Preserve emergent behavior.** Novel single-source tensions may become critical stories. Don't kill them early.
- **Anti-fragile by default.** The system should discover more when uncertain, not less.
- **Soft guidance over mechanical rules.** The LLM prompt suggests prioritization; it doesn't enforce a formula.
- **Reserve slots for the unknown.** Always leave room for low-engagement and novel tensions in the query budget.
