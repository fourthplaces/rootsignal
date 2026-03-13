# Root Signal — Editorial & Signal Inclusion Principles

This document defines what Root Signal includes, what it excludes, and why. It exists to guide every decision about source ingestion, signal classification, and feature development.

---

## Core Principle

**Root Signal surfaces local reality — what's happening, who's responding, and how to plug in.**

Every signal in the platform should answer the question: *"What is community life like here right now?"* — and wherever possible, *"What can I do?"*

Root Signal exists to surface agency, opportunity, connection, and context. It maps the full landscape of community life in a place — including tensions and crises — because understanding what's happening is the first step to showing up.

---

## The Inclusion Test

Before ingesting a source or surfacing a signal, it must pass all three:

**1. Is it community signal?**
Does it relate to community life, ecological stewardship, community engagement, ethical consumption, or the tensions and needs that animate them? Does it belong in a picture of local reality?

**2. Is it grounded?**
Is it traceable to an identifiable organization, government entity, public record, established community group, or directly reported by a person? Does it have a source?

**3. Does it connect to action or context?**
Does encountering this signal either enable someone to act (volunteer, donate, attend, advocate, steward) or help them understand what's happening in their community? Signal doesn't have to be directly actionable — context that illuminates a tension or names a pattern is also valuable — but it must connect to the signal graph.

If the answer to any of these is no, the signal doesn't belong in Root Signal.

---

## The Corrupted Information Environment

This document's inclusion test was written assuming the web is noisy but mostly honest. That assumption no longer holds.

The information environment is actively hostile. Social media platforms suppress civic content through algorithmic filtering, content removal, and API restrictions. Simultaneously, the open web is flooded with AI-generated content, coordinated astroturfing campaigns, and political noise that drowns genuine community voices. A hashtag query for a local issue may return a wall of political commentary from people with no connection to the community, bots amplifying engagement-bait, and algorithmically boosted outrage — with the actual community voices buried or removed entirely.

In this environment, open ingestion is not neutral — it's naive. Filtering for sources with verifiable real-world touchpoints isn't gatekeeping. It's quality control against a corrupted input layer. See `self-evolving-system.md` for how this shapes source discovery.

---

## The First-Hand Principle

The inclusion test's "grounded" requirement means grounded in direct experience — something happening to you, your family, your community, your neighborhood, or a request for help. Political commentary about an issue from people not personally affected by it fails the grounded test regardless of viewpoint.

This is not a political filter. It's the difference between community signal and national discourse. If an issue doesn't touch someone's life directly, it's not their community signal — it's news consumption repackaged as opinion.

Even first-hand content must still pass the full inclusion test. Root Signal's attention flows toward the wound and the response: needs, tensions, responses, gatherings, calls for help. Content that endorses or celebrates the source of harm — even from someone directly affected — doesn't surface a need, enable action, or illuminate community context. It fails the "connects to action or context" test.

The two layers work together:
1. **First-hand only** — is this person directly affected or involved?
2. **Connects to action or context** — does this surface a need, a response, or help someone understand and participate in community life?

### Scope

The first-hand filter applies specifically to **platform search/feed scraping** — the channel where political noise, AI-generated content, and astroturfing are most concentrated. It is enforced at the LLM signal extraction layer.

It does not apply to:
- **Trusted account posts** — sources already vetted for real-world involvement
- **News feeds** — already editorially filtered
- **Org websites** — meet the inclusion test by nature
- **Government/institutional sources** — grounded by definition

Platform hashtag queries are particularly vulnerable to political noise — coordinated campaigns, astroturfing, and algorithmic amplification flood results with commentary that fails both layers. The first-hand principle makes explicit what the inclusion test already implies: Root Signal captures civic reality as lived, not as debated.

---

## What We Include

**Community Needs:** Volunteer opportunities, mutual aid requests, donation drives, meal trains, mentorship programs, skill-sharing, community support networks, resource directories.

**Ecological Stewardship:** Restoration events, tree plantings, river cleanups, citizen science programs, native planting guides, community gardens, rain garden workshops, tool libraries, seed exchanges.

**Community Engagement:** Public hearings, city council meetings, open comment periods, board and commission openings, neighborhood association meetings, voter registration drives, town halls, participatory budgeting, know-your-rights workshops.

**Ethical Consumption:** Farmers markets, co-ops, local business directories, CSAs, repair events, fix-it clinics, zero-waste resources, buy-nothing groups, secondhand shops, identity-specific business directories, tool lending libraries.

**Tensions and Crises:** When local reality is under stress — ICE raids, environmental disasters, housing crises, school closures, police accountability — the graph captures both the tension and every response to it. Tension is surfaced because it's connected to action: legal aid clinics, solidarity events, GoFundMes, boycotts, policy actions, mutual aid. The tension is context. The responses are the signal.

**Evidence and Policy:** Government filings, contract data, voting records, environmental reports, court records. These are public facts that ground the graph in evidence. They surface when connected to tensions, actors, or responses — not as standalone data dumps.

---

## What We Exclude

**Threat data without community context.** Crime maps, sex offender registries, incident reports, police scanner feeds. These create fear without agency. If an organized response exists (legal aid, victim support, rights workshops, community safety initiatives), the response is the signal — not the threat data itself.

**Raw emergency alerts.** Amber alerts, severe weather warnings, active shooter notifications, evacuation orders. These are critically important but belong in purpose-built emergency systems. Root Signal is not a replacement for 911 or emergency management. However, when a crisis generates community responses (shelters, donation drives, volunteer mobilization, mutual aid), those responses are first-class signal.

**Partisan political content.** Root Signal surfaces public process (hearings, meetings, comment periods, ballot information, voting records) but never endorses candidates, parties, or positions. "When is the school board meeting and how do I testify" is in scope. "Who you should vote for" is not.

**Rumors, unverified sightings, and surveillance-style threat data.** Crime-map sightings, police scanner crowdsourcing, "suspicious person" reports, and anonymous threat tips. These create fear without agency and fail the inclusion test. Signals must trace back to an identifiable organization, government entity, or established community group — or be directly reported by a person (human-reported signal with clear provenance).

This exclusion does not apply to **community mutual-protection reporting** — community members publicly warning each other about enforcement activity, environmental hazards, or other threats where protective action is possible. When enforcement is the threat, the institutions that would normally be authoritative sources may be silent or complicit, and the community voices doing the warning *are* the signal infrastructure. These reports are valid signal — but they must earn confidence through the system's existing triangulation machinery before surfacing at high severity. A single uncorroborated social media post stays low-confidence. Multiple independent reports across channels, linked to an investigated Tension with evidence, earn escalation. The corroboration requirement is both an epistemological standard and a defense against adversarial false reports (see `adversarial-threat-model.md`).

**Personal disputes, complaints, and grievances.** Nextdoor-style neighbor complaints, landlord reviews, business callouts. Root Signal is not a reputation platform. It surfaces what's available, not what's wrong with individuals.

**Commercial advertising.** Businesses can appear in Root Signal if they meet ethical consumption criteria (local, cooperative, identity-owned, sustainable, repair-oriented). Paid placement, sponsored listings, and general business promotion are permanently out of scope.

---

## Normal Mode vs Crisis Mode

Root Signal operates in two modes, determined by the state of the graph — not by a manual toggle:

**Normal mode:** The graph reflects steady-state community life. Resources, events, ongoing needs, public processes, ecological stewardship. Most signal is affirmative — here's what's happening, here's how to participate.

**Crisis mode:** When tension clusters in the graph cross a threshold (multiple signals, same geography, same timeframe, acute urgency), the system enters crisis mode for that area. In crisis mode:
- Scraping cadence accelerates for the affected geography
- Response discovery agents actively search for who's responding
- The interface prioritizes crisis-relevant signal (shelters, legal aid, donation links, volunteer needs)
- Sensitive-signal holdback rules apply (geographic fuzziness, corroboration thresholds)
- The tension itself is surfaced as context, but always connected to responses

The boundary is consistent across both modes: **signal must be grounded and connected to action or context.** Crisis mode doesn't change what's included — it changes the urgency and the intensity of the system's attention.

---

## The Gray Zone — How to Handle Edge Cases

Some signals live at the boundary. Here's how to think about them:

**"There's a proposed development that would destroy green space near me."**
The development proposal itself is signal — it's a public process with hearings and comment periods. Include the hearing, the comment deadline, and any organized advocacy groups. Don't editorialize about whether the development is good or bad.

**"Eviction rates are high in this neighborhood."**
The statistic alone is evidence — it enters the graph as an Evidence node connected to the geography. But the highest-value signal is the response: a renters' rights clinic, a legal aid org, a tenant organizing group. Lead with the resource. The evidence provides context.

**"The food shelf on Lake Street closed."**
A closure changes the graph — a Resource node goes inactive. If another food shelf nearby is accepting new clients, or a mutual aid group has stepped in, surface that. Root Signal shows people where to go and how the landscape is shifting.

**"ICE is conducting operations in this neighborhood."**
This is a tension — and a critical one. It enters the graph connected to responses (legal aid hotlines, know-your-rights trainings, sanctuary churches, GoFundMes for affected families) and context (the federal policy, the corporate actors). The people posting about ICE activity on Reddit, Bluesky, and community forums are neighbors, organizers, and journalists who *want* this signal amplified. They are acting in the open because visibility is the point. The people who need protection — undocumented individuals and families — are not the ones broadcasting on social media. They communicate through encrypted channels, through proxies, through trusted networks. Suppressing public signal about enforcement activity doesn't protect vulnerable people — it silences the community members trying to organize a response. Root Signal treats this signal the same as any other public signal: it flows through the graph, links to responses, and is surfaced with geographic fuzziness appropriate to the sensitivity level. The signal is not held back. The community's voice is not muted.

**Pattern:** When confronted with a problem or negative condition, Root Signal surfaces both the organized, constructive response *and* the community context that helps people understand what's happening. The response is the primary signal. The context is what makes it meaningful.

---

## Tone Implications

These principles extend beyond data ingestion into how Root Signal presents information:

- Descriptions should be invitational, not alarmist
- Urgency should be about opportunity windows ("public comment closes Friday") not about threats ("they're about to approve this")
- Tension should be presented with context and connection to action, not as fear
- Language should assume the user wants to participate, not that they need to be protected
- Public community voice should be amplified, never suppressed out of fear of bad actors
- Gaps in coverage should be acknowledged honestly, not papered over with fear-based framing

**Crisis-mode exception:** When corroborated enforcement signal is linked to an active Tension — people are being detained, deported, or disappeared — direct, urgent language is appropriate. This isn't alarmism; it's accuracy. "ICE enforcement activity reported in Rosemount — legal aid resources available" is not a fear-based framing. It's a fact connected to action. The invitational tone applies to steady-state community life. When the situation is urgent and the signal is triangulated, the tone should match the reality.

---

## Applying This to Source Decisions

When evaluating a new source for ingestion, use this quick checklist:

| Question | Required Answer |
|---|---|
| Does the source carry signal — needs, resources, events, tensions, evidence, or actors? | Yes |
| Is this private content that was not intended to be public? | No — Root Signal only ingests public signal. Public broadcasts don't create new risk by being aggregated; they were already public. Geographic fuzziness applies to sensitive signals at the display layer. |
| Is the data traceable to an identifiable source? | Yes |
| Would surfacing this data in a community platform feel constructive? | Yes |

If any answer doesn't match, the source is either out of scope or needs to be ingested selectively (take the resource listings, skip the threat data).

---

## This Is a Living Document

As Root Signal grows and encounters new source types and signal categories, these principles should be revisited. The core question never changes: **Does this help someone understand and participate in community life where they are?** If yes, it belongs. If no — no matter how interesting, important, or publicly available the data is — it doesn't.
