---
date: 2026-02-24
category: ux
source: community-alert-surfacing-plan-review
---

# Low-Severity Community Report Surfacing Behavior

## The Gap

The community alert surfacing plan defines how Notices earn severity (through source trust, corroboration, and EVIDENCE_OF linkage to Tensions). But it does not specify what a Low-severity `community_report` Notice looks like to a user in the UI.

This matters because Low severity is the default state for uncorroborated, single-source community reports. The surfacing behavior creates a dilemma:

- **If Low-severity community reports appear in the UI:** A single fake ICE sighting report causes panic even before corroboration. The adversarial threat model names this exact scenario.
- **If Low-severity community reports are hidden:** Legitimate lone reports from people with first-hand knowledge are suppressed. The editorial principles say public community voice should be amplified, never suppressed.

## What Needs Deciding

- What does the user see for a Low-severity community_report? Is it visible at all?
- If visible, how does the UI communicate uncertainty? ("Unconfirmed report" label? Muted styling? Only visible in detail views, not in feeds/maps?)
- Does the Story Weaver include Low-severity Notices in story materialization, or only Medium+?
- Should Low-severity community reports be visible only when they're part of an existing Story (contextual) vs standalone?

## Related

- `docs/plans/2026-02-24-feat-community-alert-surfacing-plan.md` — severity inference section
- `docs/vision/editorial-and-signal-inclusion-principles.md` — confidence-tiered surfacing
- `docs/vision/adversarial-threat-model.md` — fake ICE report attack vector
