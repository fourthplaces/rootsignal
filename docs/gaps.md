# Root Signal: Known Data Gaps

Current blind spots in civic signal collection, organized by cause.

## Platform Censorship Gaps

| Gap | Description | Affected Domains |
|-----|-------------|-----------------|
| Algorithmic suppression | Platform feeds deprioritize civic/political content — scout sees what the algorithm wants | All social media sources |
| Content removal | Posts about sensitive topics (policing, immigration, housing) removed before or after scraping | Tensions, Needs |
| API restrictions | Apify actors get blocked or rate-limited; platforms deprecate endpoints | All social media sources |
| Shadow banning | Accounts posting civic content reduced in reach without notification | Community organizers, activists |

## Source Discovery Gaps

| Gap | Description | Affected Domains |
|-----|-------------|-----------------|
| English-language bias | Scout queries and LLM extraction are English-centric; misses non-English civic activity | All — especially immigrant communities |
| Platform bias | Heavy reliance on Instagram, Facebook, Reddit, TikTok; misses WhatsApp groups, Signal chats, Nextdoor, neighborhood listservs | Human Community Needs, Mutual Aid |
| Institutional bias | Government data and news feeds overrepresented vs. grassroots sources | Civic Engagement dominates; Ethical Consumption underrepresented |
| Geographic cold-start | New regions have no seeded sources; bootstrap queries may miss local-specific platforms or orgs | All domains in new regions |
| Offline-first communities | Communities that organize primarily offline (churches, senior centers, indigenous groups) have minimal digital footprint | Human Community Needs, Ecological Stewardship |

## Structural Gaps

| Gap | Description | Affected Domains |
|-----|-------------|-----------------|
| Ephemeral content | Stories/reels/live streams disappear before scraping cadence catches them | Tensions (protests, emergencies) |
| Private groups | Facebook groups, Discord servers, Slack workspaces are inaccessible to scraping | Mutual Aid, Community Needs |
| Dark social | Sharing via DMs, text threads, and encrypted messaging is invisible | All |
| PDF/document silos | City council agendas, budget documents, zoning filings trapped in PDFs on municipal sites | Civic Engagement |
| Event fragmentation | Events spread across Eventbrite, Facebook Events, Meetup, church bulletins, flyers — no single source | Gatherings |

## Temporal Gaps

| Gap | Description | Affected Domains |
|-----|-------------|-----------------|
| Scraping cadence | Sources checked on schedule; fast-moving events (protests, disasters) may be stale by next scrape | Tensions |
| Retroactive removal | Content scraped successfully but later removed from platform; no mechanism to detect post-hoc censorship | All |
| Seasonal patterns | Some civic activity is seasonal (elections, wildfire season, school board cycles); scout may under-weight during off-peaks | Civic Engagement, Ecological Stewardship |

## Signal vs. Noise Gaps

| Gap | Description | Affected Domains |
|-----|-------------|-----------------|
| No first-hand filter | Scout ingests political commentary alongside first-hand civic reports with no distinction; two-layer filter needed: (1) is this person directly affected? (2) does it connect to action or context? | All — especially Tensions |
| Astroturfing / coordinated noise | Hashtag queries flooded with political opinion campaigns that drown out community voices | Tensions, Civic Engagement |
| Engagement-optimized content | Platforms amplify outrage and tribalism over on-the-ground reporting | All social media sources |

## Measurement Gaps

| Gap | Description | Affected Domains |
|-----|-------------|-----------------|
| Unknown unknowns | No way to measure what the scout is missing — can't quantify the size of blind spots | All |
| Censorship visibility | No mechanism to detect when platforms suppress content (proposed in censorship-resilient brainstorm) | All social media sources |
| Community validation | No feedback loop from actual community members saying "you missed this" | All |

## Mitigation Status

| Mitigation | Status | Addresses |
|------------|--------|-----------|
| First-hand filter | Brainstormed (2026-02-21) | Signal vs. noise, astroturfing, political commentary |
| Account-following watchlists | Brainstormed (2026-02-21) | Platform censorship, algorithmic suppression |
| Censorship observatory | Brainstormed (2026-02-21) | Measurement gaps, censorship visibility |
| Human source submission | Exists (demand signals) | Community validation, offline communities |
| LLM source discovery | Exists (scout agents) | Source discovery breadth |
| Multi-language support | Not started | English-language bias |
| Private group access | Not feasible without consent | Structural gap — may remain |
