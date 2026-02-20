---
date: 2026-02-20
category: analysis
source: suppression-detection-brainstorm
---

# Active Suppression Detection

## The Gap

Rootsignal can detect what exists in the information environment. It cannot detect what was removed from it.

Information suppression — the deliberate erasure, burial, or pre-emption of content by governments, corporations, institutions, or other powerful actors — is invisible to the current system. A Wikipedia article edited to strip inconvenient context, a government report quietly revised, a dataset that goes 404, a publication pattern that abruptly stops — none of these leave a trace in rootsignal's signal graph. The system faithfully reflects the *current* state of the information environment without knowing what was taken away.

## Why This Is a Gap

**Suppression is upstream from narrative distortion.** The Divergence Analyst (see `docs/gaps/narrative-distortion-divergence.md`) detects when local narratives diverge from grounded reality elsewhere. But one of the primary *mechanisms* of that divergence is suppression — information was actively removed, so the narrative shifted. Without detecting suppression, the Divergence Analyst can observe the distortion but can't explain its cause or prove it was deliberate.

**The three forms of suppression are each invisible today:**

- **Erasure** — content that existed and was removed or materially altered. Rootsignal has no temporal record of what a page said yesterday vs. today. Once content changes, the previous version is gone.
- **Burial** — content that still exists but has been pushed out of discoverability. Scout finds what search engines surface. If a result has been SEO-buried or DMCA'd from search indices, Scout never sees it.
- **Pre-emption** — information that was never published due to legal threats, classification, gag orders, or self-censorship. The hardest form to detect, but it leaves indirect traces: FOIA redactions, topics where comparable jurisdictions have public data but this one doesn't, patterns of silence where activity is known to exist.

**Suppression has detectable signatures that no one is looking for:**

- Coordinated edits across multiple sources in a tight time window around the same topic
- Pre-event scrubbing — information cleaned up before policy announcements or legal actions
- Citation chain breaking — a source removed, then its citations removed, cascading through the reference graph
- 404 patterns clustered around specific topics or entities
- Temporal anomalies — publication schedules that skip entries, article clusters that abruptly stop

These patterns are observable in the data. The system simply doesn't look for them.

## What Exists Today

- **Web Archive (planned)** (`docs/brainstorms/2026-02-18-web-archive-brainstorm.md`): The archive crate will store every page Scout fetches as a side effect of normal operation. This creates the temporal record needed to detect erasure — but only for pages Scout happens to request. No active monitoring of sensitive sources. No diffing. No pattern detection.
- **Echo detection** (`echo.rs` in scout-supervisor): Detects manufactured signal (high volume, low type diversity). Does not detect the inverse — signal that was removed.
- **Content hash dedup**: Detects when a page hasn't changed. Does not detect when a page *has* changed materially, or why.
- **Source health checks**: Detect when a source is unreachable. Do not distinguish between a site being down and a specific page being removed.

## What's Needed

Three capabilities layered on top of the web archive:

1. **AI-driven high-cadence capture.** The archive passively stores what Scout fetches. Suppression detection requires *active* monitoring — the archive re-fetching sensitive sources on its own schedule, driven by AI that identifies suppression targets based on graph heat and topic sensitivity. Sources connected to high-tension stories get watched more closely.

2. **Temporal diffing with semantic analysis.** Comparing versions of the same URL over time to detect material changes. Not just text diffing — LLM-powered semantic analysis that distinguishes cosmetic edits from meaning-altering changes. Detects context stripping, framing shifts, methodology changes, and outright removal.

3. **Suppression pattern recognition.** Looking across multiple sources and time windows to identify suppression signatures — coordinated timing, citation chain breaking, 404 clustering, pre-event scrubbing. Individual edits are ambiguous. Patterns across sources are evidence.

The output is an evidence chain: what existed, what changed, when, and how it fits into a broader pattern. This feeds into the Divergence Analyst as concrete proof of deliberate information manipulation.

## Related

- Brainstorm: `docs/brainstorms/2026-02-20-suppression-detection-brainstorm.md`
- Web Archive: `docs/brainstorms/2026-02-18-web-archive-brainstorm.md`
- Divergence Analyst: `docs/brainstorms/2026-02-20-divergence-analyst-brainstorm.md`
- Narrative Distortion gap: `docs/gaps/narrative-distortion-divergence.md`
- Anti-fragile signal: `docs/brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md`
