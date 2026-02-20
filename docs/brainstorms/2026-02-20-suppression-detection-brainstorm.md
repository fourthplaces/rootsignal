---
date: 2026-02-20
topic: suppression-detection
---

# Suppression Detection — Surfacing Actively Buried Information

## What We're Building

A capability that detects when information has been actively suppressed — erased, buried, or pre-empted — and surfaces the evidence chain with a full paper trail. This sits upstream from the Divergence Analyst: suppression is one mechanism by which narratives become distorted, and detecting it produces concrete evidence the Divergence Analyst can use.

Suppression is distinct from distortion. Distortion adds spin. Suppression removes evidence. Both corrupt the information environment, but suppression is deliberate and leaves different traces.

## Three Forms of Suppression

**Erasure** — content that existed and was removed or materially altered. Wikipedia article revisions that strip inconvenient paragraphs. Government reports that get quietly updated. Press releases that disappear. Datasets that return 404.

**Burial** — content that still exists but has been pushed out of discoverability. SEO manipulation that buries unfavorable results. Algorithmic suppression on social platforms. DMCA takedowns that remove content from search indices but not from the source. Deliberate flooding of a topic with noise to drown out signal.

**Pre-emption** — information that never gets published. Gag orders, classification, legal threats, self-censorship. This is the hardest to detect because there's no "before" to compare against — but it leaves indirect traces: FOIA requests that come back heavily redacted, topics where comparable jurisdictions have public data but this one doesn't, patterns of silence around topics with known activity.

## Why the Web Archive Is the Foundation

The existing web-archive brainstorm (`docs/brainstorms/2026-02-18-web-archive-brainstorm.md`) describes a crate that sits between Scout and the web as a proxy layer. Every request flows through it. Every response is stored in Postgres with timestamps and content hashes.

This is exactly the infrastructure suppression detection needs. The archive creates the "before" that makes erasure visible. Without it, you can't prove something changed — you can only observe what exists now.

The key insight: **the archive already captures everything Scout sees as a side effect of normal operation.** Suppression detection extends this with:

1. **AI-driven high-cadence capture** — the archive re-fetches sensitive sources on its own schedule, not just when Scout happens to request them
2. **Temporal diffing** — comparing versions of the same URL over time to detect material changes
3. **Pattern detection** — recognizing suppression signatures across multiple sources and topics

## Architecture

```
Scout ──→ Archive (web-archive crate) ──→ Web
               │
               ↓
           Postgres
          (content store)
               │
               ↓
     Suppression Analyst
     (pattern detection + diffing)
               │
               ↓
     Divergence Analyst
     (uses suppression evidence as input)
```

The archive is the single interface to the internet. Scout never hits the web directly. This means:

- Every page Scout ever fetched is preserved automatically
- AI-driven high-cadence capture uses the same archive interface — it's just the archive re-fetching on its own initiative
- The suppression analyst queries Postgres to diff versions, detect patterns, and build evidence chains
- The longer rootsignal runs, the deeper the archive gets, the harder it becomes to suppress something without leaving a trace

## Suppression Signatures

Suppression isn't random. It follows patterns because the actors doing it operate under similar constraints and incentives. Detectable signatures include:

**Coordinated timing.** Multiple edits or removals across different sources happening in a tight window around the same topic. One Wikipedia edit is normal. Five Wikipedia edits, two deleted press releases, and a revised government report within 48 hours on the same topic — that's a pattern.

**Pre-event scrubbing.** Information gets cleaned up before a policy announcement, election, or legal action. The timing correlation between removal and public action is the signal.

**Edit clustering by actor.** The same accounts or IP ranges making material changes across multiple articles or sources related to the same topic.

**Citation chain breaking.** A source gets removed, then articles that cited it get edited to remove the citation, then articles citing those get updated. Suppression cascades through the reference graph.

**404 patterns.** Pages returning 404 clustered around specific topics or entities, especially when other pages on the same domain are fine.

**Temporal anomalies.** A cluster of articles about a topic that abruptly stops with no resolution. Publication schedules that skip entries. Datasets that stop being updated.

**Source silence.** An entity that was publicly active on a topic goes suddenly quiet. Overlaps with the anti-fragile signal concept of displaced signal detection.

## AI-Driven Capture Targeting

The archive passively captures everything Scout fetches. But suppression detection also needs active, high-cadence monitoring of sources that are likely suppression targets. The AI layer decides what to watch and how often.

**Targeting heuristics:**

- Sources connected to high-heat stories in the graph become archival targets
- Wikipedia articles on topics where rootsignal has detected political tension
- Government pages related to active policy debates or enforcement actions
- Public datasets referenced by signals in the graph
- Press releases from entities involved in contentious stories
- Court filings, regulatory documents, meeting minutes on sensitive topics

**Cadence logic:**

- Default: capture when Scout requests (passive)
- Elevated: daily re-fetch for sources connected to stories with rising heat
- High: hourly snapshots for sources showing early suppression signatures (recent edits, recent 404s on related pages)

The graph itself drives targeting. Rootsignal's own signal about what's contentious in a city tells the archive what's worth watching closely.

## Semantic Change Detection

A diff tool can tell you text changed. Only an AI can tell you the *meaning* of what changed and why it matters.

The suppression analyst needs to understand:

- **Material vs. cosmetic changes** — a typo fix is not suppression. Removing a paragraph about enforcement outcomes is.
- **Context stripping** — an article that gets edited to remove two paragraphs still exists, but meaning was removed.
- **Methodology changes** — a dataset republished with different methodology breaks comparability with previous versions. The data "exists" but the comparison was destroyed.
- **Framing shifts** — language changes that alter the implications without changing the facts. "Officers responded to a disturbance" replacing "Officers used tear gas on protesters."

This is where the LLM layer is essential. The suppression analyst uses semantic comparison, not just text diffing, to assess whether a change is material and what was lost.

## Output: Evidence Chains

When the suppression analyst detects a pattern, it produces an evidence chain — not an accusation. The chain includes:

- **The original content** — what existed, timestamped, content-hashed
- **The current content** — what exists now (or that it's gone)
- **The diff** — what specifically changed, with semantic analysis of what was lost
- **The pattern** — how this change fits into a broader suppression signature (coordinated timing, citation chain breaking, etc.)
- **The timeline** — full sequence of changes with timestamps

This evidence chain feeds into the Divergence Analyst as one category of evidence for why a local narrative may be distorted. It can also be surfaced directly to users as a paper trail.

## How It Connects to Existing Architecture

**Web Archive** (`docs/brainstorms/2026-02-18-web-archive-brainstorm.md`): The archive crate is the foundation. Suppression detection extends it with active capture targeting and temporal diffing. The `web_interactions` table already stores timestamped snapshots with content hashes — the suppression analyst queries this directly.

**Divergence Analyst** (`docs/brainstorms/2026-02-20-divergence-analyst-brainstorm.md`): Suppression evidence is an input to divergence analysis. "The local narrative diverges from grounded reality" can now include "because specific information was actively removed" with proof.

**Anti-Fragile Signal** (`docs/brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md`): Source silence and displaced signal patterns overlap with suppression detection. The anti-fragile model detects when entities go quiet; suppression detection explains *why* — the information was deliberately buried.

**Echo Detection**: Echo is manufactured signal (high volume, low type diversity). Suppression is the inverse — signal that was removed. Together they cover both sides: what was artificially added and what was artificially taken away.

## Open Questions

- How does the AI capture targeting integrate with the existing web-archive crate design? Is it a separate scheduler that calls the archive's fetch methods, or is it built into the archive itself?
- What external archives should the suppression analyst also query? Wayback Machine, archive.org, Google Cache, Lumen database (for DMCA takedowns)?
- How do we handle the legal surface area of archiving third-party content at high cadence? Is there a difference between "we fetched this page and stored it" vs. "we're systematically monitoring this source for changes"?
- How does the suppression analyst validate that a detected pattern is actually suppression vs. routine content management? Not every edit is malicious.
- What's the threshold for surfacing a suppression finding? One material edit? A pattern across multiple sources? How do we avoid false positives that erode trust in the system?
- How do we detect pre-emption (information that was never published)? This likely requires comparing what's available in one jurisdiction against comparable jurisdictions — which overlaps with the Divergence Analyst's cross-geographic comparison.

## Next Steps

→ Build the web-archive crate first (prerequisite infrastructure)
→ Design the suppression analyst as a consumer of the archive's temporal data
→ Define suppression signature patterns concretely enough to implement detection
→ `/workflows:plan` when ready to design the implementation
