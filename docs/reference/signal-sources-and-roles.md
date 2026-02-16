# Signal Sources, Audience Roles & Quality Dimensions

Extracted from the previous architecture docs as reference material for the greenfield system. The source lists, audience roles, and quality dimensions are implementation-agnostic and carry forward.

---

## Signal Sources — Tier 1 (Public, Displayable)

Content freely available on the open web. Can be scraped, stored, summarized, and surfaced with attribution and links back to the source.

### Fundraising & Mutual Aid
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **GoFundMe** | Financial need, medical, disaster relief | Web scraping (search by location) | Yes (city/zip) | Rich structured data — goal, raised, category, story |
| **GiveSendGo** | Financial need, faith-based causes | Web scraping | Limited | Smaller but active in certain communities |
| **Open Collective** | Community project funding | API (public) | By org location | Good for recurring community initiatives |
| **Mutual Aid Hub** (mutualaidhub.org) | Mutual aid networks | Web scraping | Yes (map-based) | Directory of existing mutual aid orgs |
| **GoGetFunding** | Personal fundraising | Web scraping | Limited | Smaller alternative fundraising platform |

### Volunteer Opportunities
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **VolunteerMatch** | Structured volunteer listings | Web scraping / possible API | Yes (zip + radius) | Largest volunteer database, very structured |
| **Idealist.org** | Volunteer + nonprofit jobs | Web scraping | Yes | Good for ongoing commitments |
| **HandsOn Network / Points of Light** | Volunteer events | Web scraping | Yes (local affiliates) | Often tied to local United Way chapters |
| **JustServe** | Community service projects | Web scraping | Yes (zip) | LDS-affiliated but open to all |
| **All for Good** | Aggregated volunteer opps | Web scraping | Yes | Google-backed, aggregates from multiple sources |
| **Catchafire** | Skills-based volunteering | Web scraping | Yes | Pro bono / professional skill matching |

### Events & Gatherings
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **Eventbrite** | Community events, fundraisers, workshops | API (public) | Yes (lat/lng + radius) | Well-structured, good API, category filtering |
| **Meetup** | Group gatherings, recurring meetups | API (limited free tier) | Yes | Good for recurring community groups |
| **Facebook Events** | Community events (public only) | Web scraping (public events pages) | Yes | Massive volume but scraping is fragile |
| **Patch.com** | Local news + events | Web scraping | Yes (hyperlocal) | Good for suburban/neighborhood level |
| **Library calendar systems** | Free community events | Web scraping (per library system) | Yes | Per-region — e.g., Hennepin County Library, St. Paul Public Library |
| **City/county .gov sites** | Public meetings, community events | Web scraping | Yes | Municipal government sites |
| **Community center websites** | Classes, events, programs | Web scraping | Yes | Each center is its own source |

### Organization Websites (Direct Scraping)
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **Nonprofit websites** | Volunteer pages, donation needs, events | Firecrawl / web scraping | By known org location | Core source — scrape /volunteer, /donate, /events pages |
| **Religious institution sites** | Community services, food shelves, events | Firecrawl / web scraping | By known location | Food distribution, clothing drives, community support |
| **School district websites** | Volunteer needs, supply drives | Web scraping | Yes | PTA pages, district volunteer portals |
| **Hospital/clinic community pages** | Health screenings, support groups | Web scraping | Yes | Community benefit and outreach pages |

### News & Newsletters
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **Local news sites** | Coverage of community needs, disaster response | Web scraping + search | Yes (local outlets) | Good for emerging needs / breaking situations |
| **Community-specific outlets** | Coverage of underserved communities | Web scraping | Regional | e.g., Sahan Journal for Somali/East African community in Twin Cities |
| **Independent local journalism** | Nonprofit and civic coverage | Web scraping | Regional | e.g., MinnPost in MN — varies by geography |
| **Substack / newsletters** | Community-specific digests | Email ingestion (subscribe + parse) | Varies | Many neighborhood newsletters exist on Substack |

### Government & Institutional
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **211 / United Way** | Human services directory | API or web scraping | Yes (zip) | Comprehensive but often outdated |
| **State human services departments** | State program info | Web scraping | State-level | Benefits, assistance programs |
| **FEMA disaster declarations** | Emergency response needs | API (public) | Yes (county) | Triggers emergency/crisis mode |
| **City council agendas/minutes** | Emerging community issues | Web scraping | Yes | Early signal for neighborhood-level needs |
| **USAspending** | Federal contract data | API (public) | By recipient | Entity-driven queries, no auth, 30 req/min |
| **EPA ECHO** | Environmental violations | API (public) | By facility | Two-step QID pattern: facility lookup then detailed report |

### Environmental & Ecological
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **Ocean Conservancy** | Ocean cleanup events, advocacy | Web scraping | Yes | International Coastal Cleanup is a major signal source |
| **Surfrider Foundation** | Beach cleanups, water quality, advocacy | Web scraping | Yes (chapters) | Chapter-based, strong local signal |
| **River keeper / waterkeeper orgs** | River cleanups, water monitoring, advocacy | Web scraping | Yes (watershed) | Local watershed-specific signal |
| **Land trusts** (local/national) | Land stewardship, trail maintenance, restoration | Web scraping | Yes | The Nature Conservancy, Trust for Public Land, local trusts |
| **Wildlife rehab centers** | Wildlife rescue needs, volunteer shifts | Web scraping | Yes | Per-facility, often need seasonal volunteers |
| **State DNR / Fish & Wildlife** | Habitat restoration, invasive species, citizen science | Web scraping | Yes (state/region) | MN DNR, state-level ecological programs |
| **EPA public data** | Pollution monitoring, Superfund sites, water quality | API (public) | Yes | Environmental justice screening tools (EJScreen) |
| **National Park Service** | Volunteer programs, restoration projects | Web scraping / API | Yes (park) | Volunteers-In-Parks (VIP) program |
| **iNaturalist** | Citizen science observations, biodiversity monitoring | API (public) | Yes (lat/lng) | Species observations, ecological health indicators |
| **Zooniverse** | Citizen science projects (ecology, climate, wildlife) | Web scraping / API | Varies | Research projects people can contribute to remotely |
| **Reef Check / Coral Reef Alliance** | Coral reef monitoring, restoration | Web scraping | Yes (reef location) | Dive volunteer programs, monitoring data |
| **Tree planting orgs** | Reforestation events, urban canopy projects | Web scraping | Yes | Seasonal, event-based signal |
| **Watershed districts** | Local water quality, stormwater management, cleanups | Web scraping | Yes (watershed boundary) | Hyperlocal ecological signal |
| **Climate action orgs** (350.org, Sunrise Movement, etc.) | Climate advocacy events, actions, campaigns | Web scraping | Yes (chapters) | Advocacy-oriented signal |
| **Invasive species networks** | Removal events, monitoring, reporting | Web scraping | Yes (region) | State-level invasive species councils |
| **Soil health / regenerative ag orgs** | Farm volunteering, soil restoration, education | Web scraping | Yes | Rodale Institute, local food/farm networks |
| **Marine debris trackers** (NOAA, Litterati) | Pollution data, cleanup coordination | API / web scraping | Yes (coastline/waterway) | Data-rich, geo-tagged pollution signal |

---

## Signal Sources — Tier 2 (Enrichment Only, Never Served)

Scraped via Apify. Processed for metadata extraction only. No consumer-facing output ever contains this content directly.

| Source | Enrichment Purpose | Notes |
|--------|-------------------|-------|
| **Instagram** (org accounts) | Freshness signals, capacity updates, event changes | "We're full," "event cancelled," "still need volunteers" |
| **Facebook Groups** | Emerging needs, sentiment, local chatter | Neighborhood groups, mutual aid groups, buy-nothing groups |
| **Facebook Pages** (org pages) | Org activity level, recent updates | Detects if an org is active vs. dormant |
| **X / Twitter** | Real-time updates, emergency signals | Fast-moving signal, good for crisis detection |
| **TikTok** | Community campaigns, viral fundraisers | Growing as grassroots fundraising/awareness channel |
| **Reddit** | Community discussion, emerging needs | Subreddit-specific signal about local issues |

### Tier 2 Computed Enrichment Flags
- **Freshness score** — Is this org/listing actively posting?
- **Capacity flag** — At capacity, paused, or actively seeking help?
- **Urgency signal** — Sudden spike in posts, emergency language, crisis indicators
- **Sentiment shift** — Tone change indicating a problem or surge in need
- **Event status** — Cancelled, postponed, moved, sold out
- **Verification boost** — Multiple platforms confirm the same information
- **Activity level** — Is this org alive or dormant?

---

## Signal Sources — Tier 3 (Direct Intake)

| Channel | Method | Use Case |
|---------|--------|----------|
| **SMS** (Twilio) | Dedicated phone number per geography | People text in needs or opportunities |
| **WhatsApp** (Twilio) | WhatsApp Business API | Communities that primarily use WhatsApp |
| **Email** | Inbound parse (SendGrid or similar) | Orgs forward newsletters, people email needs |
| **Web form** | Simple submission form | Standalone intake, or embedded in partner sites |
| **Voice** (Twilio + transcription) | Call-in line | Accessibility — call and describe a need |
| **QR codes** | QR to web form | Posted at libraries, community centers, churches |
| **Partner intake** | Trusted partners submit | Healthcare workers, social workers, community navigators |
| **API endpoint** | REST API | Any platform can push signal directly |

---

## Scraping Cadence

### Base Cadence by Source Type
| Source Type | Base | Ceiling | Notes |
|------------|------|---------|-------|
| Social media | 24h | 168h (7 days) | Explicit, timestamped broadcasts |
| Organization websites | 48h | 336h (14 days) | Content changes slowly |
| Search discovery | 6h | 48h | Catch new signals via targeted queries |
| Institutional databases | 168h (weekly) | 720h (30 days) | Slow-changing government data |
| Events (Eventbrite, Meetup) | 12h | — | Events posted days/weeks ahead |
| Local news | 2h | — | Breaking needs emerge fast |
| Direct intake | Real-time | — | Highest priority, process immediately |

### Adaptive Cadence
Each consecutive scrape with 0 new signals **doubles** the interval. Each scrape that produces signals **resets to base**. Sources that produce nothing naturally fade to their ceiling without human intervention.

---

## Audience Roles

How a person might act on signal:

| Role | Description |
|------|-------------|
| **volunteer** | Give time. Show up physically or virtually to do work. |
| **donor** | Give money. Contribute financially to a cause, campaign, or individual. |
| **attendee** | Show up. Be present at an event, gathering, or meeting. |
| **advocate** | Use voice and economic power. Contact representatives, sign petitions, boycott, buy differently. |
| **skilled_professional** | Give expertise. Offer specific professional skills pro bono or at reduced cost. |
| **citizen_scientist** | Give observation. Contribute to scientific understanding through data collection. |
| **land_steward** | Give care to place. Maintain, restore, or protect land, water, and ecosystems. |
| **conscious_consumer** | Give attention to impact. Change purchasing behavior, support ethical alternatives. |
| **educator** | Give knowledge. Teach, mentor, tutor, or facilitate learning. |
| **organizer** | Give coordination. Bring people together, build networks, facilitate collective action. |

---

## Signal Quality Dimensions

Every signal has implicit quality attributes:

| Dimension | Description |
|-----------|-------------|
| **Actionability** | Can someone do something concrete right now? |
| **Specificity** | How specific is the ask? "5 people who can lift 40 pounds" > "we need volunteers" |
| **Freshness** | How recently was this signal produced or confirmed? |
| **Source credibility** | Verified org, known community leader, or anonymous? Multiple sources = higher confidence. |
| **Completeness** | Does it contain everything needed to act? Location, time, what to bring, who to contact. |
| **Geographic precision** | Specific address, neighborhood, city, or location-ambiguous? |
