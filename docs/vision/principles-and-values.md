# Taproot — Principles & Values

## What This Document Is

This is the soul of the project. Every architectural decision, every product choice, every interaction with the community should be traceable back to something in this document. When there's ambiguity about the right path forward, return here.

---

## Core Belief

The distance between caring and acting should be zero. Most people want to show up for the world around them — for their neighbors, for their community, for the land and water and living systems they share. The reason they don't isn't apathy. It's friction. The information they need is scattered, stale, and buried in platforms that were designed for engagement, not for action.

Taproot exists to eliminate that friction. To make community signal — where help is needed, where life is asking for care — as findable and reliable as a utility.

---

## Principles

### 1. Signal Is a Public Good

Community signal — who needs help, where, how — is not a product. It's not content to be monetized. It's not data to be sold. It's information that belongs to the community that produced it. Taproot treats signal as a public good: freely accessible, openly available, and returned to the people who need it.

This means: open-source code, open API, no paywalls on basic signal access, no selling community data to third parties. The signal flows freely. Always.

### 2. Open Source as Commitment, Not Strategy

Taproot is open source not because it's trendy or because it's a growth hack. It's open source because the infrastructure for community care should be owned by communities, not companies. Any city, any neighborhood, any group of people should be able to run their own instance, adapt it to their needs, and maintain sovereignty over their community's signal.

This means: the codebase is public, contributions are welcome, the architecture is designed for self-hosting, and no critical functionality is locked behind proprietary services.

### 3. Utility, Not Platform

Taproot aspires to be infrastructure — reliable, always-on, boring in the best sense. Like running water or electricity. People don't think about utilities until they're gone. That's the goal. Taproot should be so reliable that communities take it for granted as the place where signal lives.

This means: uptime matters more than features. Simplicity matters more than cleverness. The API is stable and well-documented. The system does one thing — concentrate signal — and does it exceptionally well.

### 4. Serve the Signal, Not the Algorithm

Taproot does not optimize for engagement, time-on-site, clicks, or any metric that serves the platform at the expense of the person. Signal is ranked by relevance, urgency, freshness, and confidence — not by what generates the most interaction. If the most important thing happening in your community right now is boring, it still goes to the top.

This means: no algorithmic feeds designed to maximize engagement. No dark patterns. No notification spam. No gamification of helping. The signal is served straight.

### 5. Privacy as a Structural Guarantee

Taproot handles sensitive information — where people need help, who's in crisis, what communities are organizing around. Privacy isn't a feature toggle. It's a structural property of the system.

Tier 2 data (scraped from walled-garden platforms) is never served to consumers. This boundary is enforced at the database and API level, not just in application logic. Personal information from direct intake is handled with care. Location data is served at the minimum precision necessary. Taproot does not build profiles of people seeking help.

This means: privacy isn't a policy — it's architecture. The system is designed so that certain violations are structurally impossible, not merely discouraged.

### 6. Attribution and Linking Back

Taproot does not replace the sources it reads from. Every signal links back to its origin — the GoFundMe page, the org website, the Eventbrite listing. Taproot makes these sources more discoverable, not less valuable. Organizations deserve credit and traffic. Individuals deserve to have their story told in their own words, at their own URL.

This means: every signal record includes an action URL pointing to the original source. Taproot summaries are gateways, not destinations. The goal is to get people to the point of action, which usually lives somewhere else.

### 7. Tiered Trust, Structural Boundaries

Taproot operates across a spectrum of data openness — from fully public websites to login-walled social platforms. The tiering model (Tier 1: public and displayable, Tier 2: enrichment only, Tier 3: direct intake) isn't just a technical convenience. It's an ethical framework.

Tier 1 is the open web. It's public. Search engines do this. No ethical ambiguity.

Tier 2 is the gray zone. Taproot scrapes walled-garden platforms to enrich and verify public signal — detecting staleness, capacity changes, event cancellations. This data is never displayed to users. It's used to compute metadata flags (freshness score, capacity status) that attach to Tier 1 records. The ethical defensibility rests entirely on this boundary: Tier 2 content is internal signal processing, not republication.

Tier 3 is direct, consensual input. People and organizations choosing to put signal into the system. This is the cleanest tier ethically and the highest quality signal.

This means: the tiering model is non-negotiable. Tier 2 data must never leak into consumer-facing responses. If there's ever ambiguity about whether something should be displayed, it shouldn't be. When in doubt, protect the boundary.

### 8. Community Ownership Over Corporate Dependence

The platforms where community signal currently lives — Facebook, Instagram, TikTok, GoFundMe — are owned by corporations whose incentives are misaligned with community wellbeing. They optimize for engagement. They fragment information across walled gardens. They extract value from the community organizers who create content on their platforms.

Taproot exists partly as a response to this dynamic. Not to fight these platforms, but to ensure that the signal they happen to carry isn't held hostage by their business models. Communities should own their signal infrastructure. Taproot provides the tools for that ownership.

This means: Taproot is not anti-platform. It reads from these platforms because that's where signal currently lives. But it works toward a world where communities have their own infrastructure and don't depend on any corporation's continued goodwill to organize and care for each other.

### 9. Start Local, Design Global

Taproot begins in one place (Twin Cities) because proving signal quality requires depth, not breadth. But every architectural decision accounts for the possibility that this runs anywhere — a neighborhood in Portland, a crisis zone in another country, a watershed that spans three states, a coastline, a reef system.

This means: nothing is hardcoded to a specific geography. The hotspot model is flexible. The source registry is extensible. The schema supports any location on earth. Local first, but never local only.

### 10. Build in Public, Let the World Inform the Shape

Taproot is not designed in a vacuum. The product evolves through real contact with real communities, real signal, and real people trying to help. Assumptions are tested, not enshrined. The direction is clear — concentrate signal, serve it as a utility — but the path there is emergent.

This means: ship early, learn fast, listen to what the signal tells you. Don't overbuild before you've proven the foundation. Don't commit to a surface before you know what the substrate looks like. Run experiments. Let the world push back.

### 11. Life, Not Just People

Taproot serves community in the broadest sense — not just human communities but the living systems we're part of. Watersheds, forests, reefs, soil, wildlife, air, oceans. Stewardship of the land is not a secondary concern or a nice-to-have category. It's core to what Taproot is.

This means: ecological signal is first-class. Environmental stewardship opportunities sit alongside volunteer shifts and fundraisers. The heat map shows ecological hotspots alongside human ones. The taxonomy treats a wetland restoration with the same structural importance as a food shelf volunteer call.

### 12. Showing Up Takes Many Forms

Taproot recognizes that people contribute in different ways and all of them matter. Volunteering time, donating money, attending an event, boycotting a company, changing a purchasing habit, collecting scientific data, planting a tree, teaching a class, organizing a neighborhood — these are all ways of showing up. Taproot doesn't privilege one form over another.

This means: the audience role model is expansive. Signal is tagged for all the ways someone might act on it. The system doesn't assume that "helping" means physical volunteering — it includes economic action, civic participation, knowledge work, stewardship, and personal behavior change.

---

## Anti-Principles — What Taproot Will Not Do

**Taproot will not sell community data.** Signal is a public good. It is not a product.

**Taproot will not build engagement loops.** No notifications designed to pull people back in. No metrics that incentivize platform usage over real-world action.

**Taproot will not take political positions.** Taproot surfaces signal that exists — boycotts being organized, policies being debated, actions being planned. It does not editorialize. It does not endorse. It concentrates signal and lets people decide how to act.

**Taproot will not gatekeep signal.** If a community is organizing, their signal belongs in the system. The threshold for inclusion is "is this actionable and is it real?" — not "do we agree with it?"

**Taproot will not optimize for growth at the expense of trust.** Trust is the only thing that makes a community utility viable. If growth and trust ever conflict, trust wins. Every time.

**Taproot will not depend on a single corporate platform.** No critical path should run through a service that could disappear, change its API, or change its terms overnight. Diversify dependencies. Prefer open-source tooling. Self-host where possible.

**Taproot will not display Tier 2 data.** This is absolute. Content scraped from walled-garden platforms is internal enrichment data. It never appears in any consumer-facing response, under any circumstance. This boundary is the ethical foundation of the tiering model.

---

## Decision Framework

When facing an architectural or product decision, ask:

1. **Does this serve the signal or serve the platform?** If it optimizes for Taproot's growth or engagement at the expense of signal quality or community trust, don't do it.

2. **Does this respect the tiering model?** If it blurs the line between what's displayed and what's internal, tighten the boundary.

3. **Could a community run this themselves?** If the decision creates corporate dependence or makes self-hosting harder, reconsider.

4. **Does this reduce the distance between caring and acting?** If it adds friction instead of removing it, simplify.

5. **Would this still make sense at global scale?** If the decision is hardcoded to one geography or one context, generalize.

6. **Does this treat all forms of showing up with equal respect?** If it privileges one audience role over another without good reason, broaden.
