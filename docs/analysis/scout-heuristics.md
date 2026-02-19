# Scout Heuristics Analysis

Audit of hardcoded constants, magic numbers, and assumptions in `rootsignal-scout`.
Conducted 2026-02-18.

---

## Summary Scorecard

| Heuristic | Justified? | Risk |
|---|---|---|
| Bayesian weight smoothing | **Yes** — textbook | None |
| Weight-to-cadence tiers | **Yes** — matches web content reality | Low |
| Tension bonus 2.0x cap | **Yes** — product values encoded | None |
| cause_heat * 10.0 | **Yes** — unit normalization | Low (needs a comment) |
| Confidence formula 0.4/0.3/0.3 | **Partially** — compressed range [0.59–1.0] | Medium |
| Investigation adjustments | **Weak** — too small to matter | Medium |
| Dedup 0.85 / corroboration 0.92 | **Reasonable** — but model-dependent | Medium |
| Exploration 10% / 14 days | **Mostly dead path** — cadence preempts exploration | Medium |
| Budget caps | **Yes** — cost-driven | None |
| `confidence *= 0.8` geo penalty | **Weak** — arbitrary number | Low |
| `is_city_local = true` hardcoded | **No** — always-true defeats geo filter | **High** |
| Fallback strings | **No** — fabricated user-facing text | **High** |
| `scrape_count = signals_produced` | **Bug** — inflates productive source weights | **High** |

---

## 1. The Confidence Pipeline — Coherent But Compressed

Confidence flows through four stages. The handoffs are where problems hide.

### Stage 1: Extraction (`extractor.rs:202`)

Every signal starts at `confidence: 0.0`. Correct — overwritten immediately in Stage 2.

### Stage 2: Quality scoring (`quality.rs:70-71`)

```
confidence = completeness * 0.4 + geo_score * 0.3 + freshness * 0.3
```

Since `freshness_score` is always `1.0` at extraction time, the real formula on day one is:

```
confidence = completeness * 0.4 + geo_score * 0.3 + 0.3
```

A perfectly complete, geo-exact signal starts at `1.0 * 0.4 + 1.0 * 0.3 + 0.3 = 1.0`.
A bare-minimum signal (title + summary + type, no location) starts at `0.5 * 0.4 + 0.3 * 0.3 + 0.3 = 0.59`.

**The floor is 0.59, not 0.** The confidence range is compressed into [0.59, 1.0] on creation. Downstream consumers (API, UI) see numbers clustered near the top.

### Stage 3: Geo penalty (`scout.rs:1081`)

```rust
meta.confidence *= 0.8;
```

Applied to city-local sources without geo_term name matches. Pushes a bare signal to `0.59 * 0.8 = 0.47`, below the natural floor from Stage 2. The 0.8 multiplier is a gut-feel number compensating for the compressed range above.

### Stage 4: Investigation (`investigator.rs:364-391`)

- `+0.05` per DIRECT evidence (confidence >= 0.7), capped at `+0.15`
- `+0.02` per SUPPORTING evidence (confidence >= 0.5), capped at `+0.06`
- `-0.10` per CONTRADICTING evidence (confidence >= 0.7), uncapped

The pre-investigation confidence range is [0.47, 1.0] (quality scoring + geo penalty). Investigation then applies additive adjustments and clamps the result to [0.1, 1.0] (`investigator.rs:342`), so post-investigation confidence can theoretically span the full range under heavy contradiction. In practice, though, the adjustments are tiny relative to the pre-investigation range. A signal at 0.59 needs three high-confidence DIRECT evidence pieces (capped at +0.15) to reach 0.74. A single contradiction drops it by 0.10 — 66% of the maximum positive adjustment. The asymmetry is intentionally conservative (falsification > verification), but the absolute magnitudes are too small to create meaningful differentiation above the 0.47 floor in normal operation.

---

## 2. The Weight System — Well-Designed, One Bug

`compute_weight` in `scheduler.rs:191-246` is the best-designed piece in the codebase.

### Bayesian smoothing (prior=0.3, k=3)

Textbook Beta-Binomial shrinkage. With k=3, after 3 scrapes the empirical data has equal weight to the prior; by 5 scrapes the prior is fully overridden. Prevents a source that gets lucky on its first scrape from dominating.

### Tension bonus (up to 2.0x)

Rewards sources producing tension signals (the highest-value type). Directly encodes product values into scheduling priority.

### Recency decay (30-day grace, linear to 0.5 over 60 days)

Prevents thrashing while penalizing dead sources. The 0.7 factor for never-produced sources is a reasonable "benefit of the doubt."

### Diversity factor (up to 1.5x)

Rewards sources whose signals get independently corroborated. A source becomes more valuable when the system can verify it.

### Bug: `scrape_count = signals_produced` (`scout.rs:405`)

```rust
let scrape_count = source.signals_produced.max(1);
```

This passes `signals_produced` as the `scrape_count` argument to `compute_weight`. A source with 10 signals and 20 scrapes (50% yield) is evaluated as 10 signals / 10 scrapes (100% yield). This inflates the weight of every productive source.

### Initial weight 0.3 vs 0.5

Bootstrap sources (0.5) get scraped daily; discovered sources (0.3) wait 3 days. After the first scrape, `compute_weight` takes over and the initial value no longer matters. This is a policy decision (LLM-curated seeds deserve faster first chances), not a heuristic. Defensible.

---

## 3. The cause_heat * 10.0 Multiplier — Justified

Initially concerning, but reading `cause_heat.rs` clarifies: `cause_heat` is already normalized to [0.0, 1.0].

In the mechanical fallback sort (`discovery.rs:695`):

```rust
score = corroboration_count + source_diversity + cause_heat * 10.0
```

`corroboration_count` can be 5-10, `source_diversity` can be 3-8. Without 10x, a cause_heat of 0.7 contributes 0.7 — negligible next to integer counters. The 10x scales cause_heat into the same order of magnitude. It's unit normalization, not amplification.

Should be a named constant with this explanation.

---

## 4. The Dedup Thresholds — Two Different Purposes

### 0.85 — same-source dedup (`scout.rs:1257, 1312`)

"Is this semantically the same signal?" Standard industry threshold for embedding-based dedup. If an org posts the same food shelf on Instagram and Facebook, 0.85 catches it.

### 0.92 — cross-source corroboration (`scout.rs:1284, 1342`)

"Is this independently confirmed?" Higher bar because false corroboration (inflating credibility when sources aren't really confirming each other) is worse than missing a true corroboration.

### The 0.85–0.92 dead zone

Signals scoring 0.85–0.92 from different sources are neither deduplicated nor corroborated — they coexist as separate nodes about similar topics. This is correct behavior.

### Model dependency

These thresholds are untested for Voyage embeddings specifically. Different models have different similarity distributions. Recommendation: log similarity scores for a few weeks and verify the distribution matches assumptions.

---

## 5. Fabricated Fallback Strings — Genuinely Bad

`extractor.rs:245`:
```rust
availability: signal.availability.unwrap_or_else(|| "Contact for details".to_string()),
```

`extractor.rs:258`:
```rust
what_needed: signal.what_needed.unwrap_or_else(|| "Support needed".to_string()),
```

These are user-facing fabricated text. If someone sees a food shelf listed as "Contact for details" when the actual hours were just missing from the source page, the system has manufactured information. The LLM could not extract the field — it should be `None`, not a human-readable placeholder presented as real data.

---

## 6. `is_city_local = true` — Hardcoded Assumption

`scout.rs:1051-1054`:
```rust
let is_city_local = {
    true // Default to local for curated sources; discovered sources have city set
};
```

Every source is treated as city-local. Geo-filter Case 4 always triggers: signals with location names that don't match `geo_terms` get a 0.8x confidence penalty instead of being rejected.

A Serper search for "food shelf Minneapolis" could return a St. Paul result with location name "Midway" — accepted with just a 20% confidence penalty instead of being filtered out. This defeats the purpose of the geo filter for non-local results.

---

## 7. Budget Constants — Justified by Economics

Cost estimates below are rough order-of-magnitude based on Feb 2026 pricing. Actual costs vary by token count, result length, and provider plan.

**Assumptions**: Serper ~$0.01/search (standard plan), Claude Haiku ~$0.005/extraction call (short context). These should be re-validated if providers change pricing.

| Constant | Value | Est. Cost Per Run | Justification |
|---|---|---|---|
| `MAX_SEARCH_QUERIES_PER_RUN` | 10 | ~$0.10 | Safety cap; daily budget system tracks total |
| `MAX_SIGNALS_INVESTIGATED` | 5 | ~$0.20 (queries + LLM) | Prevents single phase from burning budget |
| `MAX_CURIOSITY_QUERIES` | 7 | ~$0.10 | Two discovery runs per cycle = ~$0.20 |
| `MAX_CONCURRENT_CHROME` | 2 | N/A | Railway PID/memory limits (~100MB per instance) |

---

## 8. The Exploration Policy — Mostly a Dead Path

10% exploration (`scheduler.rs:48`) with 14-day minimum staleness (`scheduler.rs:50`) and weight < 0.3 threshold (`scheduler.rs:49`).

### The policy conflict

Exploration requires a source to be **stale for 14+ days** and have weight **below 0.3**. But `cadence_hours_for_weight` (`scheduler.rs:171-180`) maps weight <= 0.2 to a 168-hour (7-day) cadence. The scheduler checks cadence first (`scheduler.rs:61`): if a source is due by cadence, it gets scheduled normally and never reaches the exploration check (`scheduler.rs:70`).

This means: a source at weight 0.15 has a 7-day cadence. It becomes due by cadence every 7 days. But exploration requires 14 days of staleness. **The source is always scheduled by cadence before it can become an exploration candidate.** The only sources that reach exploration are those with an explicit `cadence_hours` override longer than 14 days, which is not a normal path.

The 10% exploration budget, the 14-day minimum, and the deterministic staleness sort are all coherent in isolation — but the cadence-first scheduling means exploration is effectively unreachable under default cadences. This is a policy conflict that makes the exploration system a near-dead path.

### Secondary issue: deterministic cycling

When sources do reach exploration (via cadence override or edge cases), the same sources are explored in the same order every run. Dead sources cycle through repeatedly until they hit the deactivation threshold of 10 consecutive empty runs (`scout.rs:422`).

---

## 9. Quality Scoring Weights

### Geo accuracy mapping (`quality.rs:65-68`)

| Precision | Score |
|---|---|
| Exact (specific address/building) | 1.0 |
| Neighborhood | 0.7 |
| City-level or none | 0.3 |

Reasonable hierarchy. The 0.3 floor for city-level means even vague signals get some geo credit.

### Completeness (`quality.rs:50-62`)

Fraction of 6 fields populated: title, summary, signal_type (always present = 3), location, action_url, timing. A signal always scores at least 3/6 = 0.5 completeness. Combined with the 0.4 weight in the confidence formula, this contributes to the compressed range problem.

---

## 10. Investigation Evidence Thresholds

### Evidence creation threshold: 0.5 (`investigator.rs:274`)

Evidence items below 0.5 confidence are discarded entirely. This is aggressive — a piece of evidence at 0.4 might become relevant combined with future evidence. However, storing low-confidence evidence creates noise in the graph, so the tradeoff is defensible.

### Confidence thresholds for adjustment

- DIRECT requires >= 0.7 — high bar for direct confirmation
- SUPPORTING requires >= 0.5 — medium bar for contextual support
- CONTRADICTING requires >= 0.7 — matching bar for negative evidence

The symmetric threshold for DIRECT and CONTRADICTING (both 0.7) is epistemically sound. The lower bar for SUPPORTING (0.5) allows weaker evidence to contribute positively, which makes sense since supporting evidence is inherently less conclusive.

---

## Recommendations

### High Priority (Bugs)

| # | Fix | Acceptance Criteria |
|---|---|---|
| 1 | **Fix `scrape_count` in weight computation** — Use actual scrape count, not `signals_produced` | Add a `scrape_count` field to `SourceNode`. Unit test: source with 10 signals / 20 scrapes computes weight ~0.5, not ~1.0. Existing `weight_formula_bayesian_smoothing` test updated. |
| 2 | **Remove fabricated fallback strings** — Use `Option<String>` instead of "Contact for details" / "Support needed" | `availability` and `what_needed` are `Option<String>` on their respective node types. `grep -r "Contact for details\|Support needed" modules/` returns zero hits. API returns `null` for missing fields. |
| 3 | **Fix `is_city_local`** — Check source's city field against current city slug | Unit test: Serper result with non-matching location_name from a non-city source is rejected (geo_filtered incremented). Same signal from a city-matching source gets 0.8x penalty instead. |

### Medium Priority (Design Issues)

| # | Fix | Acceptance Criteria |
|---|---|---|
| 4 | **Widen confidence range** — Restructure formula so new signals span more of [0, 1] | After change, bare-minimum signals score below 0.4 and fully-complete geo-exact signals score above 0.9. Confidence distribution query shows spread, not cluster. |
| 5 | **Increase investigation adjustment magnitudes** — Or make them proportional to the confidence range | A signal at median confidence receiving 2 DIRECT evidence pieces moves by at least 0.1. Unit test covers this scenario. |
| 6 | **Log embedding similarity distributions** — Validate 0.85/0.92 for Voyage | Add histogram logging of similarity scores during dedup. After one week of data, verify: false-positive rate at 0.85 < 5%, false-negative rate at 0.92 < 10%. Adjust thresholds if needed. |
| 7 | **Fix exploration policy conflict** — Ensure low-weight sources can actually reach exploration | Either lower `exploration_min_stale_days` below the lowest default cadence (< 7 days), or exempt exploration candidates from cadence scheduling. Test: a source at weight 0.15 with `last_scraped` 10 days ago appears in exploration picks. |

### Low Priority (Cleanup)

| # | Fix | Acceptance Criteria |
|---|---|---|
| 8 | **Name the 10x constant** — `const CAUSE_HEAT_UNIT_SCALE: f64 = 10.0` with a doc comment | Constant exists, `grep "10.0" discovery.rs` no longer has an unexplained magic number. |
| 9 | **Add randomization to exploration** — Break deterministic cycling of dead sources | Exploration picks use shuffle or weighted random sampling instead of deterministic staleness sort. Two consecutive runs with the same inputs can produce different exploration picks. |
