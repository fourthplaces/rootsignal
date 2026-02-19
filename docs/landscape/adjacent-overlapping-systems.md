# Root Signal — Adjacent & Overlapping Systems

## Purpose

This document maps the landscape of existing systems, platforms, and projects that occupy territory near Root Signal. Understanding what exists — what works, what doesn't, where the gaps are — is essential for making good architectural decisions and for positioning Root Signal clearly in the world.

For each system, we assess: what it does, who it serves, what signal it carries, where it falls short, and how Root Signal relates to it.

---

## Volunteer Matching Platforms

### VolunteerMatch
**What it does:** The largest online volunteer matching platform in the US. Organizations post structured volunteer opportunities. Individuals search by location, cause, and availability.

**Signal it carries:** Structured volunteer listings with dates, skills required, time commitment, and location. High-quality, org-verified signal.

**Where it falls short:** Only captures organizations that actively post there. Heavily skewed toward larger, established nonprofits. Small grassroots groups, mutual aid networks, and informal community efforts are absent. Listings go stale — orgs post once and forget. No urgency signaling. No ecological stewardship signal. No connection to the broader landscape of need (fundraisers, events, boycotts, etc.).

**Root Signal's relationship:** VolunteerMatch is a public source. Root Signal scrapes its public listings and integrates them alongside signal from dozens of other sources. VolunteerMatch serves one audience role (volunteer) from one type of source (orgs that self-list). Root Signal serves all roles from all sources.

### Idealist.org
**What it does:** Lists volunteer opportunities, nonprofit jobs, internships, and organizations. Internationally focused.

**Signal it carries:** Volunteer opportunities, nonprofit career listings, org profiles. More international coverage than VolunteerMatch.

**Where it falls short:** Same limitations as VolunteerMatch — only orgs that self-post. Stale listings. No real-time signal. No mutual aid, no grassroots, no ecological stewardship. The job board focus dilutes the volunteer signal.

**Root Signal's relationship:** Public source. Complementary to VolunteerMatch for broader coverage, especially internationally.

### Catchafire
**What it does:** Matches skilled professionals with nonprofits for pro bono projects. Focuses on marketing, IT, HR, and strategy.

**Signal it carries:** Skills-based volunteering opportunities. Well-structured, project-scoped.

**Where it falls short:** Very narrow focus — skilled professionals only. No general volunteering, no ecological, no community action. Requires account creation and profile building. Closed ecosystem.

**Root Signal's relationship:** Public source for the `skilled_professional` audience role specifically. Root Signal makes these opportunities discoverable alongside everything else.

### Root Signal Foundation
**What it does:** Connects nonprofits with skilled volunteers for pro bono service. Focus on marketing, strategy, HR, IT.

**Signal it carries:** Pro bono project needs from nonprofits.

**Where it falls short:** Narrow scope (skills-based only). Closed platform. Name overlap — worth noting for brand awareness and clarity.

**Root Signal's relationship:** Potential public source. The name overlap is coincidental but worth addressing in communications. The Root Signal Foundation serves one slice of one audience role; Root Signal (the signal service) serves the entire spectrum.

---

## Fundraising Platforms

### GoFundMe
**What it does:** The dominant personal and community fundraising platform. Individuals and organizations create campaigns for medical bills, disaster relief, community needs, and personal causes.

**Signal it carries:** Rich, high-volume signal about community needs. Each campaign is a story of someone or something that needs help. Geographic filtering is available. Goal tracking provides natural lifecycle signals (active, funded, expired).

**Where it falls short:** GoFundMe is a fundraising tool, not a discovery tool. Finding relevant campaigns requires knowing to look. The platform optimizes for viral campaigns, not for systematic coverage of community need. Small campaigns for local needs get buried. No connection to other forms of action (volunteering, advocacy, stewardship).

**Root Signal's relationship:** GoFundMe is one of the most important public sources. It carries high-quality, geo-localized, naturally-expiring signal about community needs. Root Signal makes GoFundMe campaigns discoverable within the broader landscape of action.

### GiveSendGo
**What it does:** Faith-based crowdfunding platform. Alternative to GoFundMe with fewer content restrictions.

**Signal it carries:** Community fundraising, often faith-community oriented.

**Where it falls short:** Smaller platform, less structured data. Has become associated with politically controversial campaigns, which adds noise.

**Root Signal's relationship:** Public source. Signal is filtered through the same quality and relevance pipeline as everything else.

### Open Collective
**What it does:** Transparent fundraising for communities and open-source projects. Organizations manage finances openly.

**Signal it carries:** Ongoing community project funding. Transparent financial data.

**Where it falls short:** Niche audience. More popular with tech and open-source communities than with general community organizations.

**Root Signal's relationship:** Public source, particularly valuable for the transparency of financial data.

---

## Event Discovery Platforms

### Eventbrite
**What it does:** Event listing and ticketing platform. Organizations post events, people RSVP or buy tickets.

**Signal it carries:** Well-structured event data — date, time, location, description, category, ticket availability. Strong API. Good geographic filtering.

**Where it falls short:** Optimized for commercial events. Community events exist but are mixed in with concerts, conferences, and workshops. No distinction between "come to our marketing webinar" and "come help clean up the river." No connection to broader community need.

**Root Signal's relationship:** Public source via API. Root Signal filters for community-relevant events and categorizes them by audience role and signal domain.

### Meetup
**What it does:** Group-based recurring event platform. People form interest-based groups and schedule regular meetups.

**Signal it carries:** Recurring community gatherings, group formation signal, location-based.

**Where it falls short:** Interest-based, not values-based. Meetup groups are organized around hobbies, careers, and social activities — not around community need. The signal is about "come hang out" not "come help." Limited API access.

**Root Signal's relationship:** Selectively scraped public source. Some Meetup groups (volunteer groups, environmental groups, community engagement groups) carry relevant signal. Most don't. Root Signal needs to filter carefully.

### Facebook Events
**What it does:** Event creation and discovery within the Facebook ecosystem.

**Signal it carries:** Massive volume of community events, especially from small orgs and informal groups that don't post anywhere else.

**Where it falls short:** Walled garden. Events are trapped inside Facebook's ecosystem. Fragmented across groups, pages, and personal profiles. Difficult to scrape at scale. Many events are private or semi-private.

**Root Signal's relationship:** Public Facebook Events are scraped as first-class signal. Events inside closed groups are private and not touched. Facebook carries signal that exists nowhere else — particularly from small, informal community groups — making it important despite the access challenges.

---

## Community Information Systems

### 211 / United Way
**What it does:** National directory of health and human services. Call 211 or search online for services in your area. Covers food, housing, health, utilities, employment, and more.

**Signal it carries:** Comprehensive directory of established services. Wide geographic coverage.

**Where it falls short:** Chronically outdated. Information is entered manually by regional staff and rarely verified. Organizations change their hours, capacity, and offerings faster than 211 can track. No real-time signal. No volunteer opportunities. No events. No ecological signal. No grassroots or mutual aid coverage. The interface is often dated and difficult to use.

**Root Signal's relationship:** Public source for established service listings. Root Signal's cross-source verification — checking org social media for activity and updates — directly addresses 211's staleness problem. Where 211 is a static directory, Root Signal is a living signal system.

### Nextdoor
**What it does:** Neighborhood-based social network. Residents post to their local community — help requests, recommendations, lost pets, complaints, events.

**Signal it carries:** Hyperlocal community signal. Help requests, mutual aid, neighborhood events, safety alerts.

**Where it falls short:** Massive noise-to-signal ratio. Dominated by complaints, lost pet posts, and heated neighborhood arguments. Community help signal exists but is buried. Walled garden — requires account creation and address verification. Algorithmically sorted feed buries time-sensitive signal. Corporate-owned with ad-driven revenue model.

**Root Signal's relationship:** Very limited scraping potential (mostly behind login). Some public-facing pages carry useful signal. If accessible, the community help and event signal would be valuable, but the noise filtering required is substantial.

### Patch.com
**What it does:** Hyperlocal news and events coverage for suburban and small-town communities across the US.

**Signal it carries:** Local events, community news, some volunteer and community signal. Good geographic specificity.

**Where it falls short:** Quality varies dramatically by region. Some Patch outlets are active and well-maintained; others are ghost towns. Limited to areas where Patch has coverage. Ad-heavy experience.

**Root Signal's relationship:** Public source for events and community news where active. Useful for suburban and small-town signal that doesn't appear on bigger platforms.

---

## Ecological and Environmental Platforms

### iNaturalist
**What it does:** Citizen science platform for biodiversity observation. Users photograph organisms and AI + community help identify species. Data feeds into the Global Biodiversity Information Facility (GBIF).

**Signal it carries:** Real-time biodiversity data. Species observations with precise location, date, and identification. Research-grade observations used by scientists.

**Where it falls short:** Observation-only. No action signal — it tells you what's there but not what to do about it. Not designed for discovering volunteer opportunities or stewardship events.

**Root Signal's relationship:** Public source via API. iNaturalist data can enrich ecological hotspots with biodiversity context. An invasive species observation near a restoration site connects the "what's happening" (iNaturalist) with the "what can I do" (Root Signal).

### Zooniverse
**What it does:** The world's largest platform for people-powered research. Users contribute to real scientific research by classifying images, transcribing data, and more.

**Signal it carries:** Citizen science projects that anyone can contribute to, often remotely.

**Where it falls short:** Not geo-localized for the most part. Many projects are remote-participation only. No connection to local community action.

**Root Signal's relationship:** Public source for the `citizen_scientist` audience role. Zooniverse projects that have local geographic relevance get surfaced alongside field-based opportunities.

### Surfrider Foundation / Ocean Conservancy / River Keeper Networks
**What they do:** Environmental organizations focused on coastline, ocean, and waterway protection. Run volunteer programs, cleanups, monitoring, and advocacy.

**Signal they carry:** Beach cleanups, water quality alerts, volunteer events, advocacy campaigns. Strong local chapter structure.

**Where they fall short:** Each operates its own website and event system. Signal is fragmented across dozens of chapter sites. No unified discovery experience.

**Root Signal's relationship:** Public sources — each chapter site scraped individually. Root Signal unifies their signal into a single searchable landscape alongside all other ecological stewardship opportunities.

### EPA Environmental Justice Screening (EJScreen) / State Environmental Agencies
**What they do:** Government environmental data, monitoring, and program administration.

**Signal they carry:** Pollution data, Superfund sites, water quality reports, environmental justice indicators, public comment periods for environmental permits.

**Where they fall short:** Dense, technical, and difficult for non-specialists to parse. Action opportunities are buried in bureaucratic processes. Volunteer programs exist but are poorly publicized.

**Root Signal's relationship:** Public data source for ecological hotspot identification and enrichment. EPA data helps identify where environmental signal should be concentrated. Volunteer programs and public comment periods are actionable signal that gets extracted and served.

---

## Ethical Consumption and Corporate Accountability

### Good On You
**What it does:** Rates fashion brands on their environmental, labor, and animal welfare practices. Provides alternatives.

**Signal it carries:** Brand ratings, ethical alternatives, supply chain transparency for fashion specifically.

**Where it falls short:** Fashion-only. No coverage of food, household products, electronics, or other consumer categories. Standalone app with no connection to broader community action.

**Root Signal's relationship:** Conceptual peer for the Steward lens. Demonstrates that ethical consumption signal has demand. Root Signal's broader approach would cover all consumer categories and connect personal behavior change to community and ecological action.

### B Corp Directory
**What it does:** Certifies companies meeting high standards of social and environmental performance. Public directory of certified B Corps.

**Signal it carries:** Which companies meet ethical standards. Searchable by location and industry.

**Where it falls short:** Only covers certified companies (self-selected). Many ethical businesses aren't B Corp certified. Doesn't cover boycotts, active campaigns, or corporate accountability investigations.

**Root Signal's relationship:** Public source for positive ethical consumption signal. "Buy from these" alongside "stop buying from those."

### Buycott / Ethical Consumer
**What they do:** Apps and publications that track boycotts and ethical consumption campaigns.

**Signal they carry:** Active boycotts, product scanning, company ethics ratings.

**Where they fall short:** Niche audiences. Buycott has had maintenance issues. Ethical Consumer is UK-focused and paywalled. Neither connects personal consumption to community action.

**Root Signal's relationship:** Conceptual overlap for the `advocate` and `conscious_consumer` roles. Root Signal would aggregate this signal alongside everything else rather than requiring a separate app.

---

## Community Tech and Government

### Code for America / Community Tech Projects
**What they do:** Build technology for government and community infrastructure. Brigade network of local community tech groups.

**Signal they carry:** Open-source community tech projects, volunteer opportunities for developers, government service improvement.

**Where they fall short:** Tech-focused. The brigade model has weakened in recent years. Projects are often developer-oriented rather than community-oriented.

**Root Signal's relationship:** Philosophical alignment. Root Signal could be a Code for America brigade project or partner. Community tech volunteers are a natural audience for Signal Match.

### 311 Systems / Municipal Platforms
**What they do:** Allow residents to report issues (potholes, graffiti, broken streetlights) and request city services.

**Signal they carry:** Complaint and service-request data. Some carry community event calendars.

**Where they fall short:** Complaint-oriented, not contribution-oriented. "What's wrong" not "how can I help." No volunteer matching, no ecological signal, no mutual aid, no fundraising.

**Root Signal's relationship:** Root Signal is the positive counterpart to 311. Where 311 handles "report a problem," Root Signal handles "find a way to contribute." A city could run both, feeding them into a unified view of community need and community capacity.

### Open311 / GovTech Platforms
**What they do:** Standardized API for municipal service requests. Various government technology platforms for digital services.

**Signal they carry:** Structured municipal data. Some carry volunteer and event information.

**Where they fall short:** Adoption varies wildly by city. Most are service-delivery focused, not community-engagement focused.

**Root Signal's relationship:** Potential integration point. Cities using Open311 could feed signal into Root Signal, and Root Signal could feed signal back into municipal dashboards.

---

## Mutual Aid and Grassroots Networks

### Mutual Aid Hub (mutualaidhub.org)
**What it does:** Directory of mutual aid networks across the US. Maps where mutual aid groups exist.

**Signal it carries:** Location and contact information for mutual aid networks. Some carry current needs and offers.

**Where it falls short:** Directory-level only. Doesn't track what individual networks currently need. Static listings. Many listed networks have become dormant since the 2020 surge.

**Root Signal's relationship:** Public source for discovering mutual aid networks. Cross-source verification — checking network social media activity — can flag which networks are actually active.

### Buy Nothing Groups
**What they do:** Hyperlocal gift economy groups where neighbors give and receive items for free.

**Signal they carry:** Real-time neighbor-to-neighbor signal about what's available and what's needed in a specific area.

**Where they fall short:** Primarily on Facebook (walled garden). Each group operates independently. No aggregation across groups. No searchability from outside.

**Root Signal's relationship:** Mostly private (Facebook Groups are closed). Demonstrates the kind of hyperlocal, neighbor-to-neighbor signal that the Commons would eventually serve natively. Public-facing Buy Nothing content is fair game; closed group content is not touched.

### Community Fridges / Little Free Pantries / Mutual Aid Infrastructure
**What they do:** Physical infrastructure for community sharing — public fridges, pantries, libraries, tool libraries.

**Signal they carry:** Location and status of community sharing infrastructure. Restocking needs. Volunteer maintenance schedules.

**Where they fall short:** Information is scattered across individual Instagram accounts, Google Maps pins, and word of mouth. No unified tracking of what's stocked, what's needed, or when.

**Root Signal's relationship:** High-value public signal from multiple sources — org websites, social media accounts, map listings. Root Signal could become the first unified map of community sharing infrastructure with real-time status via cross-source verification.

---

## Charity Evaluation and Nonprofit Data

### Charity Navigator / GuideStar (Candid)
**What they do:** Rate and provide data on nonprofit organizations. Financial health, transparency, governance.

**Signal they carry:** Nonprofit financial data, organizational information, IRS data.

**Where they fall short:** Evaluation-focused, not action-focused. Tells you if an org is well-run, not what they need right now. No real-time signal. No volunteer matching. No event discovery.

**Root Signal's relationship:** Potential enrichment source. Charity Navigator data could boost confidence scores for organizations in the Root Signal system. "This org is well-rated AND they need volunteers this Saturday."

---

## The White Space — What Doesn't Exist

Root Signal occupies territory that no existing system covers:

**No system aggregates across signal types.** VolunteerMatch has volunteers. GoFundMe has fundraisers. Eventbrite has events. iNaturalist has observations. No system brings all of these together into a unified signal landscape.

**No system aggregates across platforms.** Signal is trapped in the platform where it was created. Root Signal reads from all of them.

**No system serves all audience roles.** Existing tools serve one role (volunteer, donor, attendee). Root Signal serves the full spectrum from volunteer to conscious consumer to citizen scientist.

**No system spans human and ecological signal.** Volunteer platforms don't include environmental stewardship. Environmental platforms don't include mutual aid. Root Signal treats both as first-class.

**No system provides real-time signal freshness.** Most directories go stale. Root Signal's cross-source verification and continuous re-scraping keeps signal alive.

**No system is designed as open infrastructure.** Every existing platform is a closed product. Root Signal is an API — a utility that any application can build on.

**No system connects personal behavior to community action.** Ethical consumption apps are disconnected from volunteer platforms which are disconnected from community engagement tools. Root Signal is the substrate that connects them all.

This is the gap. This is where Root Signal lives.
