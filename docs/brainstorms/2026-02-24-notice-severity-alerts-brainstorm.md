---
date: 2026-02-24
topic: notice-severity-alerts
---

# Community Alert Surfacing via Notice Severity

## What We're Building

Enhance the Notice primitive to recognize and surface urgent community warnings — particularly enforcement sightings like "ICE is in Rosemount" — by using the existing severity field, source trust, corroboration machinery, and Tension linkage.

People are already using social media for real-time mutual protection. When ICE is the threat, government is the adversary and organized groups keep their heads down to avoid targeting. Community voices *are* the signal infrastructure. Root Signal needs to recognize these patterns and surface them with appropriate urgency.

No new primitive. Alerts are Notices with high/critical severity that have earned that severity through source trust or corroboration, linked to an active Tension.

## Why This Approach

We considered introducing a new "Alert" primitive but concluded that the distinction is about severity and confidence, not type. A Notice already has `severity`, `source_authority`, and `category`. The gap is:

1. **Extraction**: The extractor doesn't recognize community mutual-protection patterns as Notices (it's tuned for institutional sources)
2. **Severity inference**: Severity is assigned by the LLM at extraction time but doesn't account for source trust, corroboration, or Tension linkage
3. **Lifecycle**: 90-day TTL is wrong for ephemeral community reports — these should decay in days, not months
4. **Surface**: Stories composed of these signals need to reflect urgency when the underlying Tension is hot and the signal is trustworthy

The existing quality machinery (source trust, corroboration count, source diversity, channel diversity, cause_heat, Tension linkage) already provides the confidence signals needed. The work is in wiring them into severity.

## Notice as Warning AND Evidence of Tension

A community post like "ICE is in Rosemount" is semantically two things at once:

- **A Notice**: The person's communicative intent is to warn — to alert the public. That's what a Notice is.
- **Evidence of an active Tension**: The post confirms that an existing Tension (ICE enforcement in the metro) is active in a specific location right now.

The graph models both through a new `EVIDENCE_OF` edge alongside the existing `RESPONDS_TO`:

| Edge | Meaning | Example |
|---|---|---|
| `RESPONDS_TO` | This signal is an action taken in response to a tension | Legal aid clinic → ICE enforcement tension |
| `EVIDENCE_OF` | This signal confirms or substantiates a tension | "ICE in Rosemount" Notice → ICE enforcement tension |
| `SOURCED_FROM` | This signal's provenance (source URL) | Any signal → Citation node |

`EVIDENCE_OF` is not limited to Notices. Any signal type can be evidence of a Tension when the relationship is true:

- A **Notice** "ICE is in Rosemount" is `EVIDENCE_OF` the ICE enforcement Tension
- A **Need** "emergency legal fund for detained families" is `EVIDENCE_OF` the same Tension — and also `RESPONDS_TO` it
- A **Gathering** "know your rights workshop" is `EVIDENCE_OF` enforcement fear — and also `RESPONDS_TO` it
- A **Citation** node (news article about raids) is `EVIDENCE_OF` the Tension

A signal can have both edges. "Emergency legal fund for detained families" is simultaneously evidence that enforcement is happening AND a response to it. Both are true. The graph says both.

### `EVIDENCE_OF` as a Structural Measure of Tension Grounding

The count of `EVIDENCE_OF` edges on a Tension becomes a direct, graph-level measure of how real and active that Tension is. This is more powerful than corroboration count on individual signals — it's the Tension itself accumulating evidence from diverse signal types and sources across the graph.

A Tension with 8 `EVIDENCE_OF` edges from Notices, Needs, Gatherings, and Citation nodes across multiple sources is very well-grounded. Signals linked to it — whether via `EVIDENCE_OF` or `RESPONDS_TO` — inherit that confidence. This feeds directly into the severity model: a Notice that is `EVIDENCE_OF` a heavily-evidenced Tension earns higher severity than one linked to a weakly-grounded Tension.

## Two Paths to High Severity

Severity is **earned**, not assigned by the LLM at extraction time. There are two independent paths to high severity — trust and corroboration:

### Path 1: Trusted Source

The system tracks source-level trust signals: `signals_produced`, `signals_corroborated`, `consecutive_empty_runs`, `weight`, and historical accuracy. A source that has consistently produced signal that gets corroborated has *already earned* credibility through the system's measurements.

**A single report from a trusted source is sufficient for high severity.** If the system has done the work to establish trust, requiring additional corroboration undermines the trust model. Trust *is* pre-accumulated corroboration.

This solves the latency problem. A trusted community account posting "ICE is in Rosemount" surfaces immediately at high severity — no waiting for a second source.

### Path 2: Corroboration (Unknown Sources)

When the source hasn't earned trust, corroboration from independent sources builds confidence:

```
1. Unknown source posts "ICE is in Rosemount" on Reddit
   → Extracted as Notice (category: community_report, severity: low)
   → Single unknown source — does not surface prominently

2. Curiosity loop fires: "Why is someone saying this?"
   → Finds existing Tension: "ICE enforcement activity in Twin Cities metro"
   → Tension already has EVIDENCE_OF edges (news articles, policy docs, prior reports)
   → EVIDENCE_OF edge wired: Notice → Tension

3. A second independent source reports the same thing
   → Another EVIDENCE_OF edge on the Tension
   → Channel diversity increases (Reddit + Twitter = 2 channels)

4. Multiple EVIDENCE_OF edges from independent sources + well-grounded Tension
   → Severity escalates to high/critical
```

### Summary

| Source Trust | Corroboration | Tension Link | Severity |
|---|---|---|---|
| Trusted | Not needed | Yes | High/Critical |
| Unknown | 2+ independent sources | Yes | High/Critical |
| Unknown | Single source | Yes | Low |
| Any | Any | No Tension context | Low |

Trust and corroboration are **two paths to the same confidence level**, not a hierarchy where you need both.

## Adversarial Defense

The threat model (`adversarial-threat-model.md`) names fake ICE sighting reports as an attack vector: submit false reports, cause panic, monitor who mobilizes to identify organized resistance.

Both paths defend against this:

- **Trusted source path**: Bad actors don't have trusted sources. Trust is earned over time through producing signal that gets independently corroborated. You can't fast-track it.
- **Corroboration path**: A single bad actor posting from one account stays low severity. Coordinated multi-platform false reports are harder to execute and more detectable. Tension linkage adds another gate — fabricated reports about activity in an area with no enforcement Tension won't link.

## Stories Are the Surface

Signals are atoms. Users don't read individual Notices — they read **Stories**.

A community alert surfaces as a **Story running hot**: a Tension (ICE enforcement) + corroborating Notices (community reports) + linked Responses (legal aid, know-your-rights, sanctuary resources). The Story Weaver materializes this into a readable narrative.

Notice severity is an **input to Story temperature**, not the user-facing output. The question isn't "how do we style a high-severity Notice?" — it's "how does a hot Story look different from a cool one?" High-severity Notices feed heat into their parent Story, which surfaces with appropriate urgency in the UI.

This means the surface work is primarily in Story display — temperature-appropriate styling, crisis-mode tone in the narrative — not in individual Notice card design.

## Key Decisions

- **No new primitive**: Alerts are Notices with earned high/critical severity
- **New Notice category**: `community_report` alongside existing `psa`, `policy`, `advisory`, `enforcement`, `health` — recognizes community mutual-protection as a valid Notice source
- **New `EVIDENCE_OF` edge**: Expresses "this signal confirms this Tension is real/active." Any signal type can be `EVIDENCE_OF` a Tension when the relationship is true. A signal can have both `EVIDENCE_OF` and `RESPONDS_TO` edges simultaneously. The count of `EVIDENCE_OF` edges on a Tension is a structural measure of its grounding.
- **Two paths to severity**: Trusted source (single report sufficient) OR corroboration from multiple independent unknown sources. Both require `EVIDENCE_OF` linkage to a Tension.
- **Shorter default TTL for all Notices**: Days, not months. Re-scrape refreshes `last_confirmed_active`, keeping persistent notices alive (fire season advisory stays on county website, gets re-confirmed each scrape). Ephemeral social media posts naturally decay when not re-scraped
- **Tension linkage required**: A Notice without an `EVIDENCE_OF` edge to an investigated Tension stays low severity regardless of source trust or corroboration. The Tension provides the evidence-backed context that makes the Notice meaningful.
- **Stories are the surface**: Notice severity feeds Story temperature. The user-facing alert is a hot Story with urgent narrative, not an individual Notice with a red icon.

## Alignment with Vision Docs

### What aligns cleanly

- **Epistemological model**: Two paths to confidence (trust, corroboration) match the existing framework. Source trust is already tracked. No special cases.
- **Tension gravity**: Notices receive cause_heat from linked Tensions — this is already the architecture. High-heat Notices feed into Story temperature naturally.
- **Crisis mode**: The editorial doc defines crisis mode triggered by tension cluster thresholds. Hot Stories composed of high-severity Notices would activate this naturally.
- **Privacy protections**: Geographic fuzziness for sensitive signals, no query logging, no organizer profiles — all structural mitigations apply unchanged.
- **No engagement loops**: Alerts surface within Stories through the existing display. No notifications, no pull-back mechanisms.
- **Emergent over engineered** (Principle 13): Severity emerges from the graph's trust and corroboration signals, not from hand-coded rules. The system detects urgency through its existing machinery.

### Editorial doc updates (completed)

The editorial principles previously excluded "rumors, unverified sightings, and crowdsourced threat reports" — which contradicted the ICE gray zone example treating community enforcement reports as valid signal. This has been resolved:

- Exclusion renamed to "rumors, unverified sightings, and surveillance-style threat data" — targeting crime maps, scanner feeds, and "suspicious person" reports
- Added carve-out for **community mutual-protection reporting** — valid signal when it earns confidence through trust or corroboration, linked to investigated Tensions
- Added **crisis-mode tone exception** — direct, urgent language is appropriate when corroborated enforcement signal is linked to active Tensions. This is accuracy, not alarmism.

## Open Questions

- **Exact TTL duration**: 3 days? 7 days? Start short and extend if persistent notices decay too fast.
- **Source trust threshold**: What measurements qualify a source as "trusted" for single-report escalation? Probably some combination of `signals_corroborated`, historical accuracy, and time-in-system. Needs design.
- **Automatic severity downgrade**: Should signal that goes quiet drop severity, or just let TTL expire?
- **Temporal inference**: Can we detect "this is happening right now" vs. "this happened yesterday"? Would help with TTL and severity.
- **Sensitivity interaction**: Enforcement warnings are `Sensitive` — geographic fuzziness applies at display. How does high Story temperature interact with geographic blur?
- **Story temperature display**: What does a hot Story actually look like vs. a cool one? Color, prominence, position, narrative tone? This is the key UX question.
- **Extraction prompting**: How much prompt work to teach the extractor to recognize community warning patterns as Notices?

## Next Steps

→ `/workflows:plan` when ready to implement
