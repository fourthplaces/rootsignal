---
date: 2026-02-20
topic: divergence-analyst
---

# Divergence Analyst — Exposing Narrative Distortion

## What We're Building

A separate agent (distinct from Scout) that takes rootsignal stories and tensions as input, investigates how the same issues are understood globally, and surfaces where the local narrative diverges from grounded reality elsewhere — and why.

Scout is a collector — it finds what exists in a city. The Divergence Analyst is an investigator — it looks at what Scout found and asks: "where does the world disagree with this, and why?" It exposes distortions, whether implicit or explicit, so individuals can make informed decisions with a clear lens instead of being tugged around by narratives with negative incentive structures (politics, news bias, cultural framing).

## Why This Matters

Most information is distorted by the incentive structures of whoever produced it. You can't act clearly on distorted information. The Divergence Analyst produces clarity by surfacing grounded counter-evidence — real outcomes from other places, real data that contradicts or complicates the local framing. Structural reality vs narrative reality.

The key word is "grounded." This isn't about presenting "the other side" (which is just more narrative). It's about finding real-world evidence from other geographies and cultures that reveals what's incomplete, contradicted, or distorted in the local picture.

## How It Differs from Scout

| Dimension | Scout | Divergence Analyst |
|---|---|---|
| Purpose | Collect signals | Investigate distortions |
| Scope | City-scoped | Globally-aware |
| Cadence | Fast, frequent | Slower, on-demand or periodic |
| Input | Web sources, RSS, social | Rootsignal's own stories/tensions |
| Output | Signals (Tension, Aid, etc.) | Divergence reports with grounded counter-evidence |
| Question | "What exists?" | "What's distorted?" |

## What It Produces

Divergence reports on specific stories, containing:
- Where the local framing diverges from how the same issue is understood elsewhere
- Grounded evidence from other geographies/cultures (real outcomes, real data, real perspectives)
- Why the narratives diverge — what incentive structures are driving the distortion
- The implicit assumptions in the local narrative that aren't universal

## Who Benefits

- **Community members** — see through local political framing with global context
- **Journalists** — divergence points are story angles
- **Researchers/policy folks** — map local tensions to global patterns and outcomes
- **Anyone making decisions** — clearer information leads to better judgment

## Key Decisions

- **Separate from Scout**: Not a Scout feature. Different cadence, different purpose, different architecture.
- **On-demand, not automatic**: Runs when pointed at specific stories/tensions, or on a slower periodic cadence. Not every signal needs global investigation.
- **Grounded, not oppositional**: Seeks real-world evidence and outcomes, not ideological counterpoints. The goal is clarity, not "balance."

## Open Questions

- What global sources does the analyst draw from? International news, academic research, government data from other countries, international NGO reports?
- How does it decide which stories/tensions warrant investigation? All of them on a slow cadence, or some selection heuristic (e.g., high-heat stories, stories with low type diversity, stories touching policy)?
- What's the output format? A report attached to the story node in the graph? A separate document? Something surfaced in the search app?
- Does it need its own graph, or does it annotate rootsignal's existing graph with divergence metadata?
- How do we validate that the counter-evidence is itself grounded and not just a different distortion?

## Next Steps

→ `/workflows:plan` when ready to design the architecture
