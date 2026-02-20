---
date: 2026-02-20
category: analysis
source: divergence-analyst-brainstorm
---

# Narrative Distortion & Cross-Cultural Divergence

## The Gap

Rootsignal tells you what's happening in a city. It does not tell you whether the picture is distorted.

Every signal rootsignal collects is produced by someone with incentives — news outlets, politicians, social media users, organizations. These incentives shape framing, emphasis, and omission. The system currently has no mechanism to detect when a local narrative diverges from how the same issue is understood in other cultures, geographies, or contexts.

## Why This Is a Gap

**The triangulation model catches echo within a story — but not across an entire information ecosystem.** A city can have excellent type diversity (tensions + responses + events) and still be operating inside a culturally distorted frame. Triangulation detects astroturfing. It doesn't detect systemic narrative bias.

**Example:** A U.S. city's signals around housing policy may surface real tensions, real organizations responding, real events — passing every triangulation check. But the entire conversation may be framed within assumptions that are contradicted by outcomes in cities that tried the same policy elsewhere in the world. The local picture is structurally complete but contextually blind.

**The distortion sources are structural, not malicious:**
- News outlets optimize for attention, not accuracy
- Political actors frame issues to serve their position
- Social media amplifies emotional resonance over grounded evidence
- Cultural assumptions are invisible to the people inside them

**Without this capability:**
- Community members make decisions based on locally-distorted information
- Policy discussions happen without evidence from other contexts
- The same mistakes get repeated because no one surfaces what happened elsewhere
- Rootsignal reproduces the information environment's biases instead of exposing them

## What Exists Today

- **Echo detection** (`echo.rs` in scout-supervisor): Catches low type diversity within a story. Does not compare against external/global context.
- **Triangulation model**: Validates structural coherence within a city's signal graph. Cannot detect when the entire graph shares a culturally biased frame.
- **Source diversity weighting**: Ensures multiple local sources contribute. Does not ensure diverse cultural or geographic perspectives.
- **Signal type rebalancing** (feedback loop 8): Corrects imbalances between tensions/responses. Does not correct framing bias.

## What's Needed

A separate agent — the **Divergence Analyst** — that takes rootsignal stories/tensions as input, investigates how the same issues are understood globally, and surfaces where the local narrative diverges from grounded reality elsewhere. It would expose distortions (implicit or explicit) so individuals can see through narrative fog and make informed decisions.

This is not "the other side." It's grounded counter-evidence: real outcomes, real data, real perspectives from other geographies that reveal what's incomplete or distorted in the local picture.

## Related

- Brainstorm: `docs/brainstorms/2026-02-20-divergence-analyst-brainstorm.md`
- Triangulation model: `docs/brainstorms/2026-02-17-triangulation-model-brainstorm.md`
- Echo detection: `modules/rootsignal-scout-supervisor/src/checks/echo.rs`
- Anti-fragile signal: `docs/brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md`
