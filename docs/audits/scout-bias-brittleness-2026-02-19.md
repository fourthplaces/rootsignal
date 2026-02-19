# Scout Module Audit: Bias, Brittleness & Inconsistencies

**Date:** 2026-02-19
**Scope:** `modules/rootsignal-scout/src/` (~3500 lines, 14 files)
**Pressure-tested against:** Architecture docs, vision docs, editorial principles, and scout heuristics docs.

## Summary

This audit identified 8 bias findings, 10 brittleness findings, and 9 inconsistency findings. After pressure testing against architecture and vision documentation, several initial findings were reclassified as intentional design decisions. This document records all findings; the companion PR fixes the true inconsistencies and replaces all unsafe code.

## Bias Findings

### B1. Hardcoded Tension Category Lists
- **Location:** `tension_linker.rs:296-297` (12 categories), `extractor.rs:438` (7 categories)
- **Description:** Two different hardcoded category lists for tension classification. The extractor uses a smaller, less granular set. Category vocabulary is baked into prompts rather than configurable.
- **Severity:** Medium
- **Status:** Fixed — unified to single `TENSION_CATEGORIES` constant in `util.rs`

### B2. Fixed Signal Taxonomy
- **Location:** `extractor.rs` system prompt
- **Description:** Signal types (Event, Give, Ask, Notice, Tension) are hardcoded. New signal types require code changes across extractor, writer, and reader.
- **Severity:** Low (by design — adding types is a deliberate architectural decision)
- **Recommendation:** Document in ADR. No code change needed.

### B3. Asymmetric Evidence Weights
- **Location:** `investigator.rs:366-391`
- **Description:** CONTRADICTING evidence penalizes 2x more than DIRECT evidence boosts (+0.05 per direct, -0.10 per contradicting). Cap on positive adjustment (0.15) but no cap on negative.
- **Severity:** Low (intentional — false positives are more harmful than false negatives)
- **Recommendation:** Document as intentional asymmetry. No code change needed.

### B4. Diffusion-Only Response Amplification
- **Location:** `response_finder.rs` investigation prompt
- **Description:** The response scout explicitly filters for responses that "diffuse" rather than "escalate" tensions. This is an editorial choice that may exclude legitimate community responses.
- **Severity:** Low (intentional editorial policy per `editorial-and-signal-inclusion-principles.md`)
- **Recommendation:** Document as editorial policy. No code change needed.

### B5. Hardcoded Topic Discovery Lists
- **Location:** `source_finder.rs` curiosity engine prompts
- **Description:** LLM-driven discovery uses hardcoded community-oriented topic categories for source expansion.
- **Severity:** Low (the LLM can add topics beyond the seed list)
- **Recommendation:** No change needed — emergent discovery compensates.

### B6. English-Only Content Processing
- **Location:** All LLM prompts across the module
- **Description:** All system prompts are English-only. Content in other languages may be poorly extracted or missed entirely.
- **Severity:** Medium
- **Recommendation:** Future work — multilingual prompt support.

### B7. Geographic Bias Toward Urban Centers
- **Location:** `scout.rs` — city-center coordinates used as defaults
- **Description:** Signals without precise geo get city-center coordinates. Rural or edge-of-city signals are spatially biased toward the center.
- **Severity:** Low (GeoPrecision::City marks these as imprecise)
- **Recommendation:** No change needed — precision field documents the limitation.

### B8. Platform Coverage Bias
- **Location:** `scraper.rs` SocialScraper implementations
- **Description:** Only Instagram, Facebook, Reddit, Twitter, and TikTok are supported. Community platforms (Nextdoor, WhatsApp groups, Signal groups) are not scraped.
- **Severity:** Medium
- **Recommendation:** Future work — add platform adapters as APIs become available.

## Brittleness Findings

### R1. Regex HTML Parsing
- **Location:** `scraper.rs:299` (`extract_links_by_pattern`)
- **Description:** Uses regex `href\s*=\s*["']([^"']+)["']` to extract links from raw HTML. Will miss href attributes with unusual quoting, whitespace, or encoding.
- **Severity:** Low (used only for link extraction from query source pages, not primary content extraction)
- **Recommendation:** Works well enough for the use case. Replace with proper HTML parser if edge cases become a problem.

### R2. Hardcoded API URLs
- **Location:** `scraper.rs:451` (Serper: `https://google.serper.dev/search`)
- **Description:** API endpoint URLs are hardcoded strings rather than configurable.
- **Severity:** Low
- **Recommendation:** Move to config if endpoint changes become frequent.

### R3. Manual Cost Estimates in Budget System
- **Location:** `budget.rs` — cost constants
- **Description:** API costs are hardcoded cents-per-operation estimates that drift as providers change pricing.
- **Severity:** Low (budget is a soft cap, not billing)
- **Recommendation:** Periodically review cost constants.

### R4. Chrome Process Management
- **Location:** `scraper.rs:53-148`
- **Description:** Spawns headless Chrome processes with retry/backoff. Resource exhaustion on constrained containers (Railway) can cascade.
- **Severity:** Medium (has retry logic and semaphore, but process leaks are possible)
- **Recommendation:** The semaphore + retry pattern is adequate. Monitor in production.

### R5. Single-Point LLM Dependency
- **Location:** All investigation and extraction code
- **Description:** Entire pipeline depends on Claude API availability. No fallback LLM provider.
- **Severity:** Medium
- **Recommendation:** Future work — provider abstraction layer.

### R6. Embedding Dimension Coupling
- **Location:** `embedder.rs`, dedup logic in `scout.rs`
- **Description:** Cosine similarity assumes fixed embedding dimensions. Changing embedding model requires re-embedding all existing vectors.
- **Severity:** Low (Voyage API version is pinned)
- **Recommendation:** Document embedding model version in config.

### R7. Apify Actor Version Pinning
- **Location:** `apify_client` crate
- **Description:** Apify actor versions may change behavior without notice.
- **Severity:** Medium
- **Recommendation:** Pin actor versions in the client.

### R8. Content Truncation at Fixed Limits
- **Location:** `extractor.rs:166-174` (30k chars), `tension_linker.rs:186-199` (8k chars)
- **Description:** Content truncation uses fixed character limits that may split mid-sentence or miss important content at the end.
- **Severity:** Low (necessary for token budget management)
- **Recommendation:** No change needed — limits are conservative.

### R9. FNV-1a Hash Collision Risk
- **Location:** `scout.rs:1984`, `investigator.rs:402`
- **Description:** FNV-1a is fast but has higher collision probability than SHA-256 for content change detection. Unlikely to matter at current scale.
- **Severity:** Low
- **Recommendation:** No change needed at current scale.

### R10. Social Platform Rate Limiting
- **Location:** `scraper.rs` — Apify-backed social scraping
- **Description:** No explicit rate limiting between social media API calls. Relies on Apify's internal rate limiting.
- **Severity:** Low (Apify handles rate limits)
- **Recommendation:** Monitor for rate limit errors in production.

## Inconsistency Findings

### I1. Diverged Tension Category Lists ✅ FIXED
- **Location:** `tension_linker.rs:296-297` (12 categories), `extractor.rs:438` (7 categories)
- **Description:** The extractor uses `housing, safety, equity, infrastructure, environment, governance, health`. The tension linker uses `housing, safety, economic, health, education, infrastructure, environment, social, governance, immigration, civil_rights, other`. Neither is a superset. "equity" maps to "economic" + "civil_rights" in the more granular vocabulary. Both populate the same `TensionNode.category: Option<String>` field, causing inconsistent categorization.
- **Severity:** Medium
- **Fix:** Unified to single `TENSION_CATEGORIES` constant in `util.rs` using the 12-category list (more granular). Removed "equity" — use "economic" and "civil_rights" instead.

### I2. Duplicated `content_hash` Function ✅ FIXED
- **Location:** `scout.rs:1984`, `investigator.rs:402`
- **Description:** Identical FNV-1a implementation in two files. If one changes, the other produces different hashes for the same content, breaking change detection.
- **Severity:** Medium
- **Fix:** Extracted to `util::content_hash()`. Both callsites now use the shared function.

### I3. Duplicated `cosine_sim_f64` Function ✅ FIXED
- **Location:** `response_finder.rs:823`, `gathering_finder.rs:786`
- **Description:** Identical f64 cosine similarity in two files. `scout.rs:129` has a separate f32 version.
- **Severity:** Low
- **Fix:** Extracted to `util::cosine_similarity()` (f64). The f32 callsite in scout.rs casts inputs.

### I4. Unsafe Transmute for Lifetime Erasure ✅ FIXED
- **Location:** `tension_linker.rs:35-58,335-340`, `response_finder.rs:261-266`, `gathering_finder.rs:287-292`
- **Description:** `SearcherHandle`/`ScraperHandle` use raw pointers with `unsafe impl Send/Sync` and `std::mem::transmute` to erase lifetimes on borrowed trait objects. While the safety invariant is documented and currently holds, this is fragile — any refactor that changes ownership could introduce use-after-free.
- **Severity:** High
- **Fix:** Replaced with `Arc<dyn Trait>` pattern. Scout now owns `Arc<dyn WebSearcher>` and `Arc<dyn PageScraper>`, passing clones to sub-scouts. Zero-cost (single atomic increment per clone). All unsafe code removed.

### I5. Hardcoded `is_ongoing` and `is_recurring` in Response Finder ✅ FIXED
- **Location:** `response_finder.rs:610` (`is_recurring: false`), `response_finder.rs:624` (`is_ongoing: true`)
- **Description:** Response finder hardcodes `is_recurring: false` for all EventNodes and `is_ongoing: true` for all GiveNodes. The gathering finder correctly uses data-driven values from the LLM (`gathering.is_recurring`).
- **Severity:** Medium
- **Fix:** Added `is_recurring: bool` to `DiscoveredResponse` struct with `#[serde(default)]`. Updated LLM prompt to extract this field. Node creation now uses `response.is_recurring` for both fields.

### I6. TikTok Content Length Filter Inconsistency ✅ FIXED
- **Location:** `scraper.rs:558-575` (`search_posts`), `scraper.rs:616-631` (`search_topics`)
- **Description:** `search_topics` applies a 20-char minimum content filter for TikTok posts; `search_posts` does not. Both methods return TikTok posts that go through the same extraction pipeline.
- **Severity:** Low
- **Fix:** Applied the same 20-char minimum filter in `search_posts` for TikTok.

### I7. Confidence 0.7 (Curiosity/Gravity) vs 0.4 (Emergent Tensions) — INTENTIONAL
- **Location:** `tension_linker.rs:576` (0.7), `response_finder.rs:734` (0.4)
- **Description:** Initially flagged as inconsistent confidence values for tension nodes.
- **Status:** Intentional design. Documented in `signal-to-response-chain.md`: "Curiosity creates tensions at 0.7 — well above the 0.5 threshold. Emergent tensions get 0.4 — below threshold. This prevents an infinite loop." Different node types from different investigation modes.

### I8. Source Weight 0.5 (Bootstrap) vs 0.3 (Discovered) — INTENTIONAL
- **Location:** `source_finder.rs:329-332` (0.5 for Curated/HumanSubmission), various (0.3 for discovered)
- **Description:** Initially flagged as inconsistent initial weights.
- **Status:** Intentional policy. Documented in `scout-heuristics.md`: "LLM-curated seeds deserve faster first chances. After the first scrape, `compute_weight` takes over and the initial value no longer matters."

### I9. DiscoveryMethod::GapAnalysis Used by Response/Gravity Scouts — INTENTIONAL
- **Location:** `response_finder.rs:792`, `gathering_finder.rs:755`
- **Description:** Response finder and gathering finder create sources with `DiscoveryMethod::GapAnalysis` even though they aren't technically gap analysis.
- **Status:** Intentional pragmatism. `get_discovery_performance()` and `get_gap_type_stats()` in `writer.rs` filter on `discovery_method IN ['gap_analysis', 'tension_seed']`. Adding new variants would make these sources invisible to the feedback loop (Loop 5) unless 3 Cypher queries, the Display impl, and deserialization are all updated. The `gap_context` field already distinguishes provenance ("Response finder: ..." / "Gathering finder: ..."). Cost exceeds benefit.
