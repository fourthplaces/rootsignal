# Root Signal — Principles & Values

## What This Document Is

This is the soul of the project. Every architectural decision, every product choice, every interaction with the community should be traceable back to something in this document. When there's ambiguity about the right path forward, return here.

---

## Core Belief

The distance between caring and acting should be zero. Most people want to show up for the world around them — for their neighbors, for their community, for the land and water and living systems they share. The reason they don't isn't apathy. It's friction. The information they need is scattered, stale, and buried in platforms that were designed for engagement, not for action.

Root Signal exists to eliminate that friction. To make community signal — where help is needed, where life is asking for care — as findable and reliable as a utility.

---

## Principles

### 1. Signal Is a Public Good

Community signal — who needs help, where, how — is not a product. It's not content to be monetized. It's not data to be sold. It's information that belongs to the community that produced it. Root Signal treats signal as a public good: freely accessible, openly available, and returned to the people who need it.

This means: open-source code, open API, no paywalls on basic signal access, no selling community data to third parties. The signal flows freely. Always.

### 2. Open Source as Commitment, Not Strategy

Root Signal is open source not because it's trendy or because it's a growth hack. It's open source because the infrastructure for community care should be owned by communities, not companies. Any city, any neighborhood, any group of people should be able to run their own instance, adapt it to their needs, and maintain sovereignty over their community's signal.

This means: the codebase is public, contributions are welcome, the architecture is designed for self-hosting, and no critical functionality is locked behind proprietary services.

### 3. Utility, Not Platform

Root Signal aspires to be infrastructure — reliable, always-on, boring in the best sense. Like running water or electricity. People don't think about utilities until they're gone. That's the goal. Root Signal should be so reliable that communities take it for granted as the place where signal lives.

This means: uptime matters more than features. Simplicity matters more than cleverness. The API is stable and well-documented. The system does one thing — concentrate signal — and does it exceptionally well.

### 4. Serve the Signal, Not the Algorithm

Root Signal does not optimize for engagement, time-on-site, clicks, or any metric that serves the platform at the expense of the person. Signal is ranked by relevance, urgency, freshness, and confidence — not by what generates the most interaction. If the most important thing happening in your community right now is boring, it still goes to the top.

This means: no algorithmic feeds designed to maximize engagement. No dark patterns. No notification spam. No gamification of helping. The signal is served straight.

### 5. Privacy as a Structural Guarantee

The system handles sensitive information — where people need help, who's in crisis, what communities are organizing around. Privacy isn't a feature toggle. It's a structural property of the system.

The boundary is **public vs. private**, not which platform something came from. Public posts on Instagram, public GoFundMe campaigns, public Facebook events — these are civic signal and are treated the same as org websites or government data. Private messages, friends-only posts, closed groups — these are never touched. Personal information from direct intake is handled with care. Location data is served at the minimum precision necessary. The system does not build profiles of people seeking help.

Privacy protection is not signal suppression. When community members post publicly about civic tensions — including enforcement activity, sanctuary responses, or organizing — they are exercising voice, not exposing vulnerability. Muting that signal out of paternalistic caution silences the people trying to help. The privacy boundary protects *private* content. Public civic voice flows freely.

This means: privacy isn't a policy — it's architecture. The system is designed so that certain violations are structurally impossible, not merely discouraged.

### 6. Attribution and Linking Back

Root Signal does not replace the sources it reads from. Every signal links back to its origin — the GoFundMe page, the org website, the Eventbrite listing. Root Signal makes these sources more discoverable, not less valuable. Organizations deserve credit and traffic. Individuals deserve to have their story told in their own words, at their own URL.

This means: every signal record includes an action URL pointing to the original source. Root Signal summaries are gateways, not destinations. The goal is to get people to the point of action, which usually lives somewhere else.

### 7. Public Signal Is Public Signal

The system reads from many platforms — org websites, social media, government databases, event platforms, fundraising sites. The ethical boundary is not which platform something came from. It's whether the content was made public by its creator.

A church posting "we need food pantry volunteers" on Instagram has the same standing as the same church posting it on their website. A GoFundMe campaign is public by design. A public Facebook event is public. The system treats all public civic signal equally — it extracts facts, builds graph nodes, and links back to the original.

What the system never touches: private messages, friends-only posts, closed groups, DMs, login-walled content that was not intended to be public. If there's ever ambiguity about whether content was meant to be public, it's left alone.

This means: the boundary is consent and intent, not platform of origin. Public broadcasts are fair game. Private content is off-limits. Attribution and linking back are non-negotiable — the system is a search engine, not a content mirror.

### 8. Community Ownership Over Corporate Dependence

The platforms where community signal currently lives — Facebook, Instagram, TikTok, GoFundMe — are owned by corporations whose incentives are misaligned with community wellbeing. They optimize for engagement. They fragment information across walled gardens. They extract value from the community organizers who create content on their platforms.

Root Signal exists partly as a response to this dynamic. Not to fight these platforms, but to ensure that the signal they happen to carry isn't held hostage by their business models. Communities should own their signal infrastructure. Root Signal provides the tools for that ownership.

This means: Root Signal is not anti-platform. It reads from these platforms because that's where signal currently lives. But it works toward a world where communities have their own infrastructure and don't depend on any corporation's continued goodwill to organize and care for each other.

### 9. Start Local, Design Global

Root Signal begins in one place (Twin Cities) because proving signal quality requires depth, not breadth. But every architectural decision accounts for the possibility that this runs anywhere — a neighborhood in Portland, a crisis zone in another country, a watershed that spans three states, a coastline, a reef system.

This means: nothing is hardcoded to a specific geography. The hotspot model is flexible. The source registry is extensible. The schema supports any location on earth. Local first, but never local only.

### 10. Build in Public, Let the World Inform the Shape

Root Signal is not designed in a vacuum. The product evolves through real contact with real communities, real signal, and real people trying to help. Assumptions are tested, not enshrined. The direction is clear — concentrate signal, serve it as a utility — but the path there is emergent.

This means: ship early, learn fast, listen to what the signal tells you. Don't overbuild before you've proven the foundation. Don't commit to a surface before you know what the substrate looks like. Run experiments. Let the world push back.

### 11. Life, Not Just People

Root Signal serves community in the broadest sense — not just human communities but the living systems we're part of. Watersheds, forests, reefs, soil, wildlife, air, oceans. Stewardship of the land is not a secondary concern or a nice-to-have category. It's core to what Root Signal is.

This means: ecological signal is first-class. Environmental stewardship opportunities sit alongside volunteer shifts and fundraisers. The heat map shows ecological hotspots alongside human ones. The taxonomy treats a wetland restoration with the same structural importance as a food shelf volunteer call.

### 12. Showing Up Takes Many Forms

Root Signal recognizes that people contribute in different ways and all of them matter. Volunteering time, donating money, attending an event, boycotting a company, changing a purchasing habit, collecting scientific data, planting a tree, teaching a class, organizing a neighborhood — these are all ways of showing up. Root Signal doesn't privilege one form over another.

This means: the audience role model is expansive. Signal is tagged for all the ways someone might act on it. The system doesn't assume that "helping" means physical volunteering — it includes economic action, civic participation, knowledge work, stewardship, and personal behavior change.

---

## Anti-Principles — What Root Signal Will Not Do

**Root Signal will not sell community data.** Signal is a public good. It is not a product.

**Root Signal will not build engagement loops.** No notifications designed to pull people back in. No metrics that incentivize platform usage over real-world action.

**Root Signal will not take political positions.** Root Signal surfaces signal that exists — boycotts being organized, policies being debated, actions being planned. It does not editorialize. It does not endorse. It concentrates signal and lets people decide how to act.

**Root Signal will not gatekeep what enters the graph.** If a community is organizing, their signal belongs in the system. The threshold for inclusion is "is this civic, grounded, and connected to action or context?" — not "do we agree with it?" The system practices open ingestion and confidence-tiered surfacing: everything civic enters the graph, but what surfaces first is determined by evidence density, freshness, and source corroboration — and every ranking factor is visible to the user.

**Root Signal will not optimize for growth at the expense of trust.** Trust is the only thing that makes a community utility viable. If growth and trust ever conflict, trust wins. Every time.

**Root Signal will not depend on a single corporate platform.** No critical path should run through a service that could disappear, change its API, or change its terms overnight. Diversify dependencies. Prefer open-source tooling. Self-host where possible.

**The system will not touch private content.** Private messages, friends-only posts, closed groups, DMs — these are off-limits regardless of how accessible they might be technically. The boundary is what the creator intended to be public. This is absolute.

---

## Decision Framework

When facing an architectural or product decision, ask:

1. **Does this serve the signal or serve the platform?** If it optimizes for Root Signal's growth or engagement at the expense of signal quality or community trust, don't do it.

2. **Does this respect the public/private boundary?** If it blurs the line between public civic signal and private content, tighten the boundary.

3. **Could a community run this themselves?** If the decision creates corporate dependence or makes self-hosting harder, reconsider.

4. **Does this reduce the distance between caring and acting?** If it adds friction instead of removing it, simplify.

5. **Would this still make sense at global scale?** If the decision is hardcoded to one geography or one context, generalize.

6. **Does this treat all forms of showing up with equal respect?** If it privileges one audience role over another without good reason, broaden.
