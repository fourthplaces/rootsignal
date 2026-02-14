# Root Signal — Kill Test: What Could Go Wrong

## Purpose

This document tries to kill Root Signal. Every assumption, every risk, every failure mode — laid out plainly so they can be confronted before they become surprises. If the system survives this document, it's worth building. If it doesn't, better to know now.

Each section describes a failure mode, explains why it could kill the project, and proposes a test or mitigation. Some of these are existential. Some are speed bumps. They're labeled accordingly.

---

## Signal Quality Failures

### 1. The Dead Feed Problem
**Risk level: Existential**

You scrape 10 sources, extract 40 signals, and the resulting feed feels dead. Stale volunteer listings from 6 months ago. GoFundMes that already reached their goal. Events that already happened. An org that shut down last year but their website is still up. A person visits, sees a wall of irrelevant or outdated information, and never comes back.

**Why this kills it:** Trust is destroyed on the first visit. A community utility that serves bad information is worse than no utility at all. People will forgive an empty feed ("it's new, give it time") but they won't forgive a feed full of dead signal. You get one shot at first impression.

**Test:** After first sprint, manually review every signal in the database. For each one, ask: "If I showed up to act on this today, would it work?" Track the percentage that are genuinely actionable. If it's below 70%, the pipeline needs fundamental work before anything else.

**Mitigation:** Aggressive expiration defaults. Tier 2 freshness checking from day one, not as a later enhancement. Build "last confirmed active" as a core field, not a nice-to-have. Default to hiding low-confidence signals rather than showing everything.

---

### 2. Insufficient Signal Volume
**Risk level: Existential**

You solve the freshness problem but there simply aren't enough active, actionable signals in a local area to make the experience feel alive. The Twin Cities has 3.7 million people in the metro — but maybe the scrapeable signal for "how to help right now" on any given day is only 20 items. Twenty items isn't a utility. It's a sad list.

**Why this kills it:** A sparse feed communicates "nothing is happening" even if lots is happening — it's just not captured yet. Users will conclude the tool doesn't work rather than concluding the sources are incomplete.

**Test:** Before building any pipeline, do a manual audit. Spend 4 hours across all known sources and count how many actionable, current signals you can find for the Twin Cities right now, today. If you can find 100+, volume isn't the problem. If you can find fewer than 30, you need more sources or a different geography.

**Mitigation:** Expand source list aggressively. Use Tavily discovery queries to find signal in unexpected places. Consider broadening the geographic radius. Supplement with ongoing/recurring opportunities (not just one-time events) to keep volume up. Consider whether "things you can do at home" and evergreen stewardship content can fill gaps without feeling like filler.

---

### 3. Extraction Hallucinations
**Risk level: High**

The LLM extraction layer (Claude) misinterprets raw content and produces bad structured data. A news article about a food shelf closing gets extracted as a volunteer opportunity. A fundraiser for a scam gets extracted as legitimate. An org's "about us" page gets turned into a fake event. A church's historical page about a 2019 event gets extracted as upcoming.

**Why this kills it:** Bad extractions are worse than no extractions. A person who shows up to volunteer based on a hallucinated signal loses trust permanently. At scale, extraction errors compound — every bad signal degrades the entire feed.

**Test:** Run extraction on 100 raw pages and manually grade every output. Check: is the signal type correct? Is the timing right? Is the location right? Is the action URL valid? Is this actually actionable? Track error rate by field. If any critical field (signal type, timing, location) has an error rate above 10%, the extraction prompts need rework.

**Mitigation:** Two-pass extraction with self-verification. Have the LLM extract, then have a second pass verify the extraction against the source. Flag low-confidence extractions for human review rather than auto-publishing. Build a feedback loop where bad signals can be reported and used to improve extraction prompts.

---

### 4. Duplication Overload
**Risk level: Medium**

The same volunteer opportunity appears on VolunteerMatch, the org's website, their Facebook page, and an Eventbrite listing. Without effective dedup, the feed is full of duplicates that make it feel like a broken search engine rather than a curated signal feed.

**Why this kills it:** Duplication is noise. It makes the feed feel spammy and untrustworthy. It inflates volume artificially, masking the real signal density.

**Test:** After first sprint, count unique signals vs total extracted signals. If the dedup pipeline is catching fewer than 60% of actual duplicates (verified manually), it needs work.

**Mitigation:** Multi-layer dedup — URL matching, fuzzy title+org matching, and vector similarity. Merge duplicates into single records with multiple sources (which actually increases confidence). Prioritize the richest source as the primary record.

---

### 5. Geo-Localization Failures
**Risk level: High**

A signal gets tagged to the wrong location. A national org's page gets tagged to their headquarters instead of the local chapter. A GoFundMe for someone in Minneapolis gets tagged to the donor's location in Chicago. A "Twin Cities area" signal gets pinpointed to a random centroid that's in the middle of a lake.

**Why this kills it:** Location is the primary filter. If someone searches for their neighborhood and gets results from across the country, the experience is immediately broken. Geographic precision is foundational to the value proposition.

**Test:** After extraction, manually verify locations on 50 signals. Check: is the pin in the right place? Is it in the right neighborhood? Is it in the right city? Track geo-accuracy rate. If it's below 80%, the geo pipeline needs significant work.

**Mitigation:** Multi-step geo resolution with confidence scoring. Fall back to broader geography (city-level) rather than guessing at street-level. Flag signals with low geo-confidence for review. Build neighborhood boundary polygons for the first hotspot so "Northeast Minneapolis" resolves correctly.

---

## Source Access Failures

### 6. Walled Gardens Block Access
**Risk level: High**

Facebook aggressively blocks scrapers. Instagram requires login. Nextdoor is fully walled. GoFundMe implements anti-bot measures. The sources with the richest signal become inaccessible.

**Why this kills it:** If the most valuable signal sources are unscrapeable, the feed is skewed toward only what's on the open web — which is dominated by larger, more established organizations. Grassroots, informal, and community-level signal (which is the most valuable) lives disproportionately on walled platforms.

**Test:** Before building automated scrapers, manually test access to every planned Tier 1 and Tier 2 source. Can you get the data? How many requests before you get blocked? Is the data structured enough to extract from? Document the access profile for each source.

**Mitigation:** Apify for Tier 2 (they handle the cat-and-mouse game with platforms). Firecrawl for Tier 1 websites. Accept that some sources will be intermittently accessible and build the system to gracefully degrade. Prioritize sources you can reliably access over sources with theoretically better signal. Long-term, direct intake (Tier 3) reduces dependence on scraping.

---

### 7. Source Fragility and Breakage
**Risk level: Medium**

Websites change their HTML structure. APIs change their response format. A scraper that worked yesterday breaks today. At scale with dozens of sources, something is always broken.

**Why this kills it:** Maintenance burden compounds. If you're spending all your time fixing scrapers, you're not improving the product. Signal from broken scrapers silently stops flowing, creating dead zones in the feed that users notice as "nothing happening in [category]."

**Test:** After building 5 scrapers, track how often they break over 30 days. If more than 2 break per week, the maintenance model is unsustainable.

**Mitigation:** Build scrapers defensively — fail loudly when structure changes. Monitor signal volume per source and alert when it drops unexpectedly. Use Firecrawl's LLM-based extraction over CSS selectors where possible (more resilient to HTML changes). Accept that some sources will have higher maintenance costs and prioritize accordingly.

---

### 8. Legal / Terms of Service Risk
**Risk level: Medium**

A platform sends a cease-and-desist. GoFundMe changes their robots.txt. An org complains that their content is being scraped without permission.

**Why this kills it:** Probably doesn't kill it outright, but could force removal of key sources. The reputational risk of being seen as a "scraper" rather than a "community utility" could undermine trust.

**Test:** This isn't testable in advance, but you can de-risk it. Document the legal landscape (hiQ v. LinkedIn precedent). Ensure Tier 2 boundary is airtight. Ensure attribution and linking back is impeccable.

**Mitigation:** The tiering model is the primary defense. Tier 1 is defensible — it's public web content, same as what Google does. Tier 2 never surfaces to users. Attribution and action URLs send traffic back to sources, which makes the relationship additive, not extractive. Long-term, build direct relationships with orgs who want to be in the system (Tier 3), reducing dependence on scraping entirely.

---

## Trust and Content Failures

### 9. Bad Actors and Scam Signal
**Risk level: High**

Someone creates a fake GoFundMe and it gets scraped into Root Signal. A scam org puts up a volunteer page that's really a data harvesting scheme. A malicious actor posts fake mutual aid requests to collect money.

**Why this kills it:** A single scam surfaced through Root Signal damages trust catastrophically. People will blame Root Signal for the scam even though Root Signal just aggregated publicly available information. The "we just surface what's out there" defense doesn't hold when someone loses money.

**Test:** Deliberately include known-scam examples in the extraction pipeline and verify that confidence scoring flags them. Search for common scam patterns in the data (newly created campaigns, no org verification, suspicious language patterns).

**Mitigation:** Confidence scoring with multiple factors — source platform trust, org verification status, cross-source confirmation, campaign age, language patterns. Never surface low-confidence signals without clear disclaimers. For financial signals (donation/fundraiser), apply extra scrutiny. Link back to original platforms where users can see reviews, comments, and social proof. Long-term, the Org Dashboard with verification becomes the trust layer.

---

### 10. Politically Sensitive Signal
**Risk level: Medium**

Root Signal surfaces an economic boycott related to a divisive political issue. Or a fundraiser for a cause that half the community opposes. Or an advocacy action that one side views as harmful. People accuse Root Signal of being politically biased.

**Why this kills it:** A community utility needs broad trust across political lines. If Root Signal is perceived as left-leaning or right-leaning, it loses half its potential audience. The "we just surface signal" neutrality stance is hard to maintain when the signal itself is politically charged.

**Test:** Review a week's worth of extracted signal and flag everything that could be perceived as politically charged. Assess: is the balance roughly reflective of what's actually happening in the community? Or is the source list skewed in a way that over-represents one political orientation?

**Mitigation:** The Principles doc is clear — Root Signal doesn't editorialize, endorse, or gatekeep. It surfaces signal that exists. But the source list and extraction criteria need to be genuinely balanced. If you scrape progressive advocacy orgs, you should also scrape conservative faith-based service orgs. The inclusivity has to be real, not performative. Anticipate criticism and have a clear, public statement about neutrality ready.

---

### 11. Privacy Violations
**Risk level: High**

A mutual aid request gets scraped that contains someone's home address, phone number, or medical situation. A GoFundMe reveals sensitive personal details that the creator didn't expect to be aggregated elsewhere. Someone's crisis gets surfaced in a way they didn't consent to.

**Why this kills it:** Privacy violation in a community-serving context is devastating. The people Root Signal is trying to help — those in crisis, those seeking aid — are the most vulnerable to privacy harm.

**Test:** Run extraction on 50 mutual aid and fundraiser signals. Check every output for PII — names, addresses, phone numbers, medical details, immigration status. If any PII leaks through that shouldn't, the extraction pipeline needs a PII scrubbing step.

**Mitigation:** Build PII detection into the extraction pipeline. Strip personal details from summaries. Link back to the original source where the creator controls their own disclosure. Never scrape or surface signal from private conversations, closed groups, or DMs. For Tier 3 direct intake, build clear consent language about what will be public.

---

### 12. Harmful Signal Amplification
**Risk level: Medium**

Root Signal inadvertently amplifies a fraudulent charity, a predatory "volunteer" opportunity that's actually unpaid labor exploitation, a dangerous amateur wildlife rescue operation, or a gathering organized by a hate group under the guise of community service.

**Why this kills it:** Amplifying harm is the opposite of Root Signal's mission. If the system can't distinguish genuine community signal from exploitative or dangerous signal, it becomes a vector for harm rather than a tool for good.

**Test:** Research known examples of exploitative volunteer schemes, fraudulent charities, and bad-faith community events. Feed representative examples through the extraction pipeline and check whether confidence scoring or categorization catches them.

**Mitigation:** Build a known-bad-actor registry over time. Use confidence scoring aggressively for new, unverified sources. For categories with high harm potential (financial, in-person events with vulnerable populations), apply stricter thresholds. Enable community reporting of bad signal and build a feedback loop.

---

## Adoption and Usage Failures

### 13. Nobody Comes
**Risk level: Existential**

You build a beautiful signal pipeline, the data is good, the freshness is real, and nobody uses it. No organic discovery. No word of mouth. The utility sits there serving signal into the void.

**Why this kills it:** Obvious.

**Test:** Put the first sprint output (even as a basic HTML page or JSON feed) in front of 10 people in the Twin Cities who you know are community-active. Ask two questions: "Would you check this regularly?" and "Would you share this with someone?" If fewer than 6 out of 10 say yes to both, the signal quality or presentation isn't good enough.

**Mitigation:** The first audience is not "the general public." It's community organizers, mutual aid network leaders, and neighborhood association members — people who are already doing this work manually. If Root Signal saves them time, they'll use it and tell others. Find 10 of these people and make them your first users. Build for them before building for everyone.

---

### 14. The Cold Start Problem
**Risk level: High**

Even if the signal is good, a new Root Signal instance in a new city has no signal until the scrapers run, the extraction pipeline processes, and enough data accumulates to feel useful. The first user in Portland opens Root Signal and sees nothing. They leave and don't come back.

**Why this kills it:** Every new hotspot deployment faces this. If the bootstrapping period is too long or too visibly empty, the utility fails before it starts.

**Test:** Time the bootstrapping process. From "spin up a new hotspot" to "100 actionable signals in the database," how long does it take? If it's more than 48 hours, the bootstrapping needs to be faster.

**Mitigation:** Pre-seed new hotspots. Before launching a new city, run the scrapers in batch mode and fill the database. Never launch an empty instance. Also, include ongoing/recurring opportunities and evergreen stewardship content that provides baseline volume even before real-time scraping catches up.

---

### 15. Org Hostility
**Risk level: Medium**

Nonprofits and community organizations react negatively to being scraped. They see Root Signal as taking their content without permission, competing for their audience, or misrepresenting their work. Instead of becoming allies, they become adversaries.

**Why this kills it:** Orgs are the primary producers of Tier 1 signal. If they actively block scraping or publicly complain, it undermines both the data pipeline and community trust.

**Test:** Reach out to 5 local nonprofits. Show them how their information appears in Root Signal. Ask: "Is this helpful or harmful to you?" Listen carefully to the response.

**Mitigation:** Attribution is everything. Every signal links back to the org. Root Signal sends traffic to them, not away from them. Frame the relationship as "we make you more discoverable" not "we take your content." Long-term, the Org Dashboard gives them ownership and control. Early outreach and relationship building with key local orgs is critical.

---

## Sustainability Failures

### 16. Cost Escalation
**Risk level: Medium**

Claude API costs grow as signal volume increases. Apify costs grow with more Tier 2 sources. Scraping frequency multiplied by number of hotspots creates a cost curve that outpaces any revenue or funding.

**Why this kills it:** An infrastructure project with no revenue model that costs $500/month for one hotspot and $5,000/month for ten hotspots burns through goodwill and personal funds fast.

**Test:** Track actual costs during first sprint. Extrapolate: what would 5 hotspots cost? 20? At what point is this unsustainable on personal funds?

**Mitigation:** Optimize extraction — batch processing, smart re-scraping (only re-process changed content), use smaller models where full Claude isn't needed. Explore grant funding (civic tech grants, community foundation grants). Consider a sustainability model where cities or institutions pay for managed hotspot instances while the core remains open and free.

---

### 17. Maintenance Burden Exceeds Capacity
**Risk level: High**

Solo developer maintaining 30 scrapers, an extraction pipeline, a database, an API, enrichment jobs, expiration logic, and monitoring. Something breaks every day. The project becomes a full-time ops job instead of a product development effort.

**Why this kills it:** Burnout. The project dies not from a technical failure but from one person drowning in maintenance.

**Test:** After 30 days of running the first sprint, track how many hours per week go to maintenance vs new development. If maintenance exceeds 50%, the architecture needs to be simpler or more resilient.

**Mitigation:** Build for resilience from day one. Scrapers should fail gracefully and loudly. Use managed services where possible. Resist adding new sources until existing ones are stable. Consider which sources give the most signal per maintenance dollar and ruthlessly cut the rest.

---

### 18. Scope Creep Paralysis
**Risk level: High**

The vision is so broad — human services, ecology, ethical consumption, boycotts, citizen science, global heat maps — that every sprint feels like it should tackle the next exciting thing instead of finishing the current thing. Nothing gets polished. Everything is 60% done.

**Why this kills it:** A system that does 20 things poorly is worse than one that does 3 things well. Users need to trust that the core experience works before they'll explore edges.

**Test:** After each sprint, ask: "Does the core experience — finding actionable local signal — work noticeably better than it did two weeks ago?" If the answer is consistently no because effort went to new categories or new sources instead of improving existing ones, scope creep is winning.

**Mitigation:** The first sprint spec exists for a reason. GoFundMe, Tavily, 5 org websites, Eventbrite. That's it. Don't add ecological sources until human services signal is proven. Don't add boycott signal until volunteer signal is solid. Each layer earns the right to the next by being good, not by existing.

---

## Systemic Risks

### 19. Platform Dependency Inversion
**Risk level: Medium**

Root Signal becomes dependent on one or two scraping tools (Firecrawl, Apify) that change their pricing, degrade their service, or shut down. The utility that was supposed to free signal from platform dependence becomes dependent on scraping platforms.

**Why this kills it:** Single points of failure in the supply chain undermine the resilience the whole project is built on.

**Test:** Identify every third-party service in the pipeline. For each one, ask: if this disappeared tomorrow, could I replace it within a week?

**Mitigation:** Abstract scraping behind a common interface so individual tools can be swapped. Have fallback options identified for every critical service. For the most important sources, consider building custom scrapers that don't depend on third-party services at all.

---

### 20. The "Good Enough Google" Problem
**Risk level: Medium**

Someone argues: "I can just Google 'volunteer Minneapolis' and find what I need. Why do I need Root Signal?" If the marginal improvement over a Google search isn't dramatically obvious, adoption stalls.

**Why this kills it:** Root Signal has to be meaningfully better than the status quo, not just slightly better. Utility adoption requires a step-change in value, not an incremental improvement.

**Test:** Do a side-by-side comparison. Google "volunteer near me" and compare the first 20 results to the first 20 Root Signal signals. Is Root Signal noticeably more actionable, more current, more specific, more local? If a neutral observer can't tell the difference, the value proposition isn't landing.

**Mitigation:** The differentiator isn't just "more results" — it's freshness (is this actually current?), specificity (is this in my neighborhood, not just my city?), multi-type aggregation (volunteers AND fundraisers AND events in one place), and audience role filtering (show me what matches how I want to help). These things must be obviously better than Google from the first interaction.

---

### 21. AI Platform Risk
**Risk level: Low-Medium**

The extraction pipeline depends on Claude API. Anthropic changes pricing, rate limits, or model behavior in ways that break extraction quality or economics.

**Why this kills it:** If extraction quality degrades or costs spike, the core pipeline suffers.

**Test:** Not testable in advance, but manageable.

**Mitigation:** Abstract the LLM layer so models can be swapped. Test extraction quality with alternative models periodically. Keep extraction prompts version-controlled and regression-tested. If Anthropic becomes untenable, the same prompts should work with other providers.

---

## Pre-Launch Checklist

Before showing Root Signal to anyone outside the core team, verify:

- [ ] **Signal actionability rate exceeds 70%** — at least 7 out of 10 signals are genuinely actionable today
- [ ] **Signal volume exceeds 50 active signals** for the first hotspot — the feed feels alive, not empty
- [ ] **No stale signals older than 30 days** appear without explicit "ongoing" designation
- [ ] **Zero PII leaks** in consumer-facing signal summaries
- [ ] **Geo-accuracy exceeds 80%** — pins are in the right neighborhood
- [ ] **Dedup catches at least 60%** of actual duplicates (verified manually)
- [ ] **Extraction error rate below 10%** on critical fields (signal type, timing, location)
- [ ] **Every signal has a working action URL** that leads to the real opportunity
- [ ] **Tier 2 data is confirmed absent** from all API responses (structural, not just checked)
- [ ] **The feed is noticeably better than Googling** the same query — side-by-side test with 3 people
- [ ] **Five community-active people** have reviewed the output and said "I would use this"
- [ ] **Cost per signal is known** and projected for 5x and 20x scale
- [ ] **Scrapers have been stable** for at least 14 consecutive days without manual intervention
- [ ] **At least one "wow" signal** exists — something a user couldn't have easily found on their own
