---
date: 2026-02-24
category: security
source: community-alert-surfacing-plan-review
---

# Cross-Tension Amplification Attack

## The Gap

The community alert surfacing plan introduces a mechanism where EVIDENCE_OF Notices boost Tension heat, which feeds into Situation temperature, which can trigger crisis-mode behavior (accelerated scraping, response discovery). The plan includes a logarithmic scaling cap with a 3x hard ceiling per Tension to prevent runaway heat from a viral community report.

However, this cap is per-Tension, not per-Situation. A coordinated adversary could fabricate multiple Tensions in the same geography, each with moderate evidence from seemingly diverse sources, collectively pushing a Situation into crisis mode without any single Tension exceeding the cap.

## Attack Scenario

1. Adversary creates accounts across multiple platforms (Reddit, Twitter, community forums)
2. Posts enforcement sighting reports for different locations within the same area, each slightly different
3. System extracts multiple Notices, curiosity loop creates or links to multiple Tensions
4. Each Tension gets modest EVIDENCE_OF boost (within the 3x cap)
5. But the Situation spanning that geography now has N hot Tensions, collectively pushing temperature past crisis threshold
6. Crisis mode activates: accelerated scraping, response discovery — adversary monitors who mobilizes

## What Needs Deciding

- Should crisis-mode activation require institutional/editorial source confirmation, not just community_report Notices?
- Should there be a Situation-level cap on heat contribution from community_report Notices specifically?
- Is the existing `source_diversity` check sufficient to catch coordinated multi-account attacks?
- Should the supervisor have a "crisis mode audit" that flags rapid Situation temperature spikes driven primarily by community reports?

## Related

- `docs/plans/2026-02-24-feat-community-alert-surfacing-plan.md` — cause_heat boost section
- `docs/vision/adversarial-threat-model.md` — bad-faith data submitters, fake ICE reports
- `docs/vision/editorial-and-signal-inclusion-principles.md` — crisis mode definition
