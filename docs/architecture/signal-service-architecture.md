# Taproot — Architecture

## The Engine

Underneath Taproot is a general-purpose signal engine. It crawls sources, extracts structured data via AI, geo-localizes it, deduplicates it, and serves it through an API. The engine itself is domain-agnostic — it doesn't know or care whether it's extracting volunteer opportunities, real estate listings, or academic papers. What makes it do a specific thing is configuration:

- **Sources** — what to crawl (a registry of URLs, APIs, and intake channels)
- **Extraction prompts** — what to look for in raw content (the AI instructions that turn HTML into structured records)
- **Taxonomy** — what signal types, categories, and audience roles exist (the classification schema)
- **Hotspots** — where to focus geographically (boundaries and scraping cadence)

All of these are data, not code. The engine is the same regardless of intent. The configuration is what makes it serve a specific purpose.

## What Taproot Is

Taproot is one configuration of this engine — pointed at community and ecological signal. It answers one question at any geographic scale: **where is life asking for help, and how can people show up?**

This includes human community signal (volunteer needs, fundraisers, mutual aid, events), ecological signal (habitat restoration, pollution monitoring, wildlife rescue, land stewardship, climate action), and accountability signal (corporate behavior, environmental violations, institutional transparency). The pipeline is signal-agnostic — the same mechanics apply whether the need is a food shelf in Minneapolis or a coral reef restoration project in the Pacific.

It is not an application. It is not a website. It is infrastructure — an API that any application, community, or institution can plug into to access concentrated, actionable, local signal.

```
Sources ──→ Engine (crawl, extract, geo-locate, dedup, store) ──→ API ──→ Consumers
                          ▲
                    Configuration
              (prompts, taxonomy, sources, hotspots)
```

Taproot is the configuration. The engine is reusable. What gets built on top of the API is documented in the ecosystem doc.

---

## Core Concepts

### Signal
A discrete, actionable piece of community information: a need, an opportunity, a call for help, an event, a fundraiser, a volunteer shift. Something a person could act on.

### Hotspots
Geographic concentrations of signal. A neighborhood, a city, a crisis zone. Hotspots can be any radius — a 5-block area after a tornado, the Twin Cities metro, or a region experiencing a humanitarian crisis. The service is geography-agnostic by design; the Twin Cities is the first hotspot, not the only one.

### Audience Roles
How someone might act on a signal: volunteer time, donate money, attend an event, offer a professional skill, spread the word, provide physical goods.

### Lenses
Different consumers see the same signal differently. A healthcare worker referral tool, a city emergency dashboard, a weekly neighborhood digest, a global heat map for crisis response — these are all lenses on the same substrate.

---

## Signal Tiering Model

### Tier 1 — Public, Displayable
Content freely available on the open web. Can be scraped, stored, summarized, and surfaced directly to consumers with attribution and links back to the source.

### Tier 2 — Semi-Public, Enrichment Only
Content behind soft walls (login-required but publicly joinable platforms). Scraped via Apify or similar. **Never served to consumers.** Used exclusively as internal metadata to boost, flag, or annotate Tier 1 content — freshness signals, capacity updates, sentiment shifts, event status changes.

**Architectural enforcement:** Tier 2 data is stored in a separate table (`signal_enrichments`) with a `tier` column. The API serving layer filters on `tier = 1` for all consumer-facing responses. Tier 2 data only manifests as computed flags (freshness score, capacity status, confidence boost) on Tier 1 records. This boundary is structural, not just policy.

### Tier 3 — Direct Intake
Signal that enters the system directly from people or organizations — via SMS, web forms, email, voice, or from connected community platforms. Highest quality signal, most actionable.

---

## Signal Sources — Tier 1 (Public, Displayable)

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
| **Local news sites** | Coverage of community needs, disaster response | Web scraping + Tavily search | Yes (local outlets) | Good for emerging needs / breaking situations |
| **Community-specific outlets** | Coverage of underserved communities | Web scraping | Regional | e.g., Sahan Journal for Somali/East African community in Twin Cities |
| **Independent local journalism** | Nonprofit and civic coverage | Web scraping | Regional | e.g., MinnPost in MN — varies by hotspot |
| **Substack / newsletters** | Community-specific digests | Email ingestion (subscribe + parse) | Varies | Many neighborhood newsletters exist on Substack |
| **Google Alerts** | Keyword-triggered news | Email ingestion | Configurable | Set alerts per hotspot + category |

### Government & Institutional
| Source | Signal Type | Ingestion Method | Geo-Filterable | Notes |
|--------|------------|-----------------|----------------|-------|
| **211 / United Way** | Human services directory | API or web scraping | Yes (zip) | Comprehensive but often outdated |
| **State human services departments** | State program info | Web scraping | State-level | Benefits, assistance programs |
| **FEMA disaster declarations** | Emergency response needs | API (public) | Yes (county) | Triggers emergency/crisis hotspot mode |
| **City council agendas/minutes** | Emerging community issues | Web scraping | Yes | Early signal for neighborhood-level needs |
| **UN OCHA / ReliefWeb** | International humanitarian needs | API (public) | Yes (country/region) | For global hotspot expansion |

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
| **Tree planting orgs** (One Tree Planted, Arbor Day Foundation, local groups) | Reforestation events, urban canopy projects | Web scraping | Yes | Seasonal, event-based signal |
| **Watershed districts** | Local water quality, stormwater management, cleanups | Web scraping | Yes (watershed boundary) | Hyperlocal ecological signal |
| **Climate action orgs** (350.org, Sunrise Movement, local groups) | Climate advocacy events, actions, campaigns | Web scraping | Yes (chapters) | Advocacy-oriented signal |
| **Invasive species networks** | Removal events, monitoring, reporting | Web scraping | Yes (region) | State-level invasive species councils |
| **Soil health / regenerative ag orgs** | Farm volunteering, soil restoration, education | Web scraping | Yes | Rodale Institute, local food/farm networks |
| **Marine debris trackers** (NOAA, Litterati) | Pollution data, cleanup coordination | API / web scraping | Yes (coastline/waterway) | Data-rich, geo-tagged pollution signal |

---

## Signal Sources — Tier 2 (Enrichment Only, Never Served)

Scraped via Apify or similar. Processed for metadata extraction only. No consumer-facing output ever contains this content directly.

| Source | Enrichment Purpose | Ingestion Method | Notes |
|--------|-------------------|-----------------|-------|
| **Instagram** (org accounts) | Freshness signals, capacity updates, event changes | Apify Instagram scraper | "We're full," "event cancelled," "still need volunteers" |
| **Facebook Groups** | Emerging needs, sentiment, local chatter | Apify Facebook scraper | Neighborhood groups, mutual aid groups, buy-nothing groups |
| **Facebook Pages** (org pages) | Org activity level, recent updates | Apify Facebook scraper | Detects if an org is active vs. dormant |
| **X / Twitter** | Real-time updates, emergency signals | Apify Twitter scraper | Fast-moving signal, good for crisis detection |
| **TikTok** | Community campaigns, viral fundraisers | Apify TikTok scraper | Growing as grassroots fundraising/awareness channel |
| **Reddit** | Community discussion, emerging needs | API (public, limited) or Apify | Subreddit-specific signal about local issues |
| **LinkedIn** (org pages) | Org hiring, program expansion signals | Apify LinkedIn scraper | Indicates org capacity/growth |

### Tier 2 → Computed Enrichment Flags
Tier 2 data is distilled into structured metadata that attaches to Tier 1 records:

- **Freshness score** — Is this org/listing actively posting? When was their last update?
- **Capacity flag** — At capacity, paused, or actively seeking help?
- **Urgency signal** — Sudden spike in posts, emergency language, crisis indicators
- **Sentiment shift** — Tone change indicating a problem or surge in need
- **Event status** — Cancelled, postponed, moved, sold out
- **Verification boost** — Multiple platforms confirm the same information = higher confidence
- **Activity level** — Is this org alive or dormant?

---

## Signal Sources — Tier 3 (Direct Intake)

### SMS / Messaging
| Channel | Method | Use Case |
|---------|--------|----------|
| **Twilio SMS** | Dedicated phone number per hotspot | People text in needs or opportunities |
| **Twilio WhatsApp** | WhatsApp Business API | Communities that primarily use WhatsApp |

### Email
| Channel | Method | Use Case |
|---------|--------|----------|
| **Dedicated intake email** | Email parsing (SendGrid Inbound Parse or similar) | Orgs forward newsletters, people email needs |
| **Newsletter subscriptions** | Auto-subscribe to local newsletters, parse incoming | Automated ingestion of curated local content |

### Web / API
| Channel | Method | Use Case |
|---------|--------|----------|
| **API endpoint** | REST API for programmatic submission | Any platform can push signal directly |
| **Web form** | Simple submission form | Standalone intake, or embedded in partner sites |

### Voice
| Channel | Method | Use Case |
|---------|--------|----------|
| **Twilio Voice + Whisper/Deepgram** | Call-in line, transcribe, extract signal | Accessibility — call and describe a need |

### Physical / Analog Bridge
| Channel | Method | Use Case |
|---------|--------|----------|
| **QR codes** | QR → web form | Posted at libraries, community centers, churches |
| **Partner intake** | Trusted partners submit on behalf of others | Healthcare workers, social workers, community navigators |

### Connected Community Platforms
| Channel | Method | Use Case |
|---------|--------|----------|
| **Community platforms** | API integration | Direct posts from connected platform users become Tier 3 signal |
| **Other community platforms** | API integration | Any forum, Circle, Discord bot, or community app that connects |

---

## Extraction & Normalization Pipeline

### Step 1: Raw Ingestion
Each source has a dedicated scraper/connector that produces raw content.

```
Source → Scraper/Connector → Raw Content Store (PostgreSQL jsonb)
```

**Tools:**
- **Firecrawl** — Website scraping, handles JS-rendered pages
- **Apify** — Social media scraping (Tier 2), marketplace of pre-built scrapers
- **Tavily** — Search-based discovery (pointed queries per hotspot)
- **Twilio** — SMS/WhatsApp/Voice ingestion
- **SendGrid Inbound Parse** — Email ingestion
- **RSS feeds** — Where available (news sites, org blogs)
- **Platform APIs** — Eventbrite, Meetup, FEMA, ReliefWeb, etc.

### Step 2: AI Extraction (Claude)
Raw content → structured signal. The crawler and extraction pipeline are intent-agnostic — the same mechanics apply whether the content is a volunteer listing, a fundraiser, an EPA violation record, or a church's public financial disclosures. The extraction prompt directs what to look for; the pipeline doesn't care what kind of signal it's extracting.

This means extraction has two dimensions for any entity Taproot encounters:

- **Opportunity signal** — "What does this organization need? What can people do here?" (volunteer shifts, donation needs, events, stewardship projects)
- **Accountability signal** — "What should people know about this entity before engaging?" (environmental violations, labor practices, financial irregularities, discrimination policies, lawsuits, lobbying activity)

Both are first-class signal. A church that runs a food shelf and also has a discrimination policy produces two signals. A corporation with a Superfund site and a community giving program produces two signals. Taproot surfaces both and lets people decide. The same extraction pipeline handles both — the only difference is the prompt's intent.

Two-pass system:

**Pass 1 — Analytical Extraction:**
```json
{
  "signal_type": "See signal-taxonomy.md for full list of signal types",
  "title": "extracted or generated title",
  "summary": "what is being asked for, in plain language",
  "organization": "who is behind this (if applicable)",
  "location": {
    "address": "if available",
    "neighborhood": "extracted or inferred",
    "city": "city name",
    "region": "state/province/region",
    "country": "country code",
    "lat": 44.9778,
    "lng": -93.2650,
    "radius_relevant": "neighborhood | city | metro | region | national | global"
  },
  "timing": {
    "start": "ISO date if applicable",
    "end": "ISO date if applicable",
    "recurring": true,
    "urgency": "immediate | this_week | this_month | ongoing | flexible"
  },
  "audience_roles": ["volunteer", "donor", "attendee", "advocate", "skilled_professional", "citizen_scientist", "land_steward", "conscious_consumer", "educator", "organizer"],
  "categories": "See signal-taxonomy.md for full list of categories",
  "action_url": "link back to original source for taking action",
  "source": {
    "url": "original URL",
    "platform": "gofundme | org_website | eventbrite | ...",
    "scraped_at": "ISO timestamp",
    "tier": 1
  },
  "confidence": 0.0
}
```

**Pass 2 — Enrichment via Tier 2:**
Tier 2 data matched to Tier 1 records by organization name, URL, or content similarity. Produces enrichment flags only.

### Step 3: Geo-Localization
Resolution pipeline for content without clean addresses:

1. **Explicit address** → geocode via Nominatim (open source) or Google Geocoding API
2. **Organization name** → lookup in org table → known location
3. **Neighborhood/district mention** → map to known boundary polygons
4. **Zip/postal code** → centroid
5. **City-level only** → flag as city-wide
6. **No location** → attempt inference from context, flag for review if low confidence

### Step 4: Deduplication
Same opportunity often appears across multiple sources:

1. **URL-based dedup** — same source URL = same record, merge metadata
2. **Fuzzy title + org + date matching** — similar listings across platforms
3. **Vector similarity** (pgvector) — catch semantically identical listings with different wording
4. **Merge strategy** — keep richest record, note all source URLs, highest confidence wins

### Step 5: Storage

```sql
-- Core signal record
signals
├── id (uuid)
├── signal_type (enum)
├── title (text)
├── summary (text)
├── organization_name (text)
├── location (geography point)
├── neighborhood (text)
├── city (text)
├── region (text)
├── country (text, ISO 3166)
├── geo_confidence (float)
├── radius_relevant (enum)
├── timing_start (timestamptz)
├── timing_end (timestamptz)
├── urgency (enum)
├── audience_roles (text[])
├── categories (text[])
├── action_url (text)
├── confidence (float)
├── freshness_score (float, computed from Tier 2)
├── capacity_status (enum: open | limited | full | unknown)
├── embedding (vector)
├── is_active (boolean)
├── created_at (timestamptz)
├── updated_at (timestamptz)
└── expires_at (timestamptz)

-- Raw source data per signal (supports multiple sources per signal)
signal_sources
├── id (uuid)
├── signal_id (uuid → signals)
├── source_url (text)
├── platform (enum)
├── tier (int)
├── raw_content (jsonb)
├── scraped_at (timestamptz)
└── scraper_version (text)

-- Tier 2 enrichment data (never served to consumers)
signal_enrichments
├── id (uuid)
├── signal_id (uuid → signals)
├── enrichment_type (enum: freshness | capacity | urgency | sentiment | event_status | verification | activity_level)
├── value (jsonb)
├── source_platform (enum)
├── tier (int, always >= 2)
├── detected_at (timestamptz)
└── confidence (float)

-- Organization registry (built over time)
organizations
├── id (uuid)
├── name (text)
├── website (text)
├── location (geography point)
├── city (text)
├── region (text)
├── country (text)
├── social_urls (jsonb)
├── last_seen_active (timestamptz)
└── verified (boolean)

-- Hotspot definitions
hotspots
├── id (uuid)
├── name (text)
├── description (text)
├── center (geography point)
├── radius_meters (int)
├── boundary (geography polygon, optional for irregular shapes)
├── hotspot_type (enum: city | neighborhood | metro | crisis_zone | region | watershed | coastline | ecosystem | migration_corridor | custom)
├── is_active (boolean)
├── created_at (timestamptz)
└── signal_density (float, computed)
```

---

## API Surface (The Service Interface)

Taproot exposes a read API for consumers and a write API for producers.

### Read API (for consumers — lenses, apps, dashboards)
```
GET /signals
  ?lat=44.97&lng=-93.26&radius_km=10    # geographic filter
  ?hotspot_id=uuid                        # or filter by named hotspot
  ?audience_role=volunteer                 # filter by how someone wants to help
  ?categories=food_security,housing        # filter by need category
  ?urgency=immediate,this_week             # filter by urgency
  ?signal_type=volunteer_opportunity       # filter by type
  ?min_confidence=0.7                      # quality threshold
  ?since=2026-02-01T00:00:00Z             # temporal filter
  ?limit=50&offset=0                       # pagination

GET /signals/{id}                          # single signal detail

GET /hotspots                              # list active hotspots
GET /hotspots/{id}                         # hotspot detail with signal summary
GET /hotspots/{id}/heatmap                 # signal density data for visualization

GET /stats
  ?hotspot_id=uuid                         # signal volume, category breakdown, trends
```

### Write API (for producers — partner orgs, community platforms, direct intake)
```
POST /signals                              # submit new signal (Tier 3)
  { signal_type, title, summary, location, timing, audience_roles, ... }

PATCH /signals/{id}                        # update existing signal
DELETE /signals/{id}                        # mark signal as resolved/expired

POST /hotspots                             # define a new hotspot
```

---

## Scheduling & Orchestration

### Scraping Cadence
| Source Type | Frequency | Rationale |
|------------|-----------|-----------|
| GoFundMe local search | Every 6 hours | New fundraisers appear daily |
| Org websites (volunteer/donate pages) | Daily | Content changes slowly |
| Eventbrite / Meetup | Every 12 hours | Events posted days/weeks ahead |
| Local news | Every 2 hours | Breaking needs emerge fast |
| Tier 2 social media | Every 12 hours | Enrichment doesn't need to be real-time |
| Government / 211 | Weekly | Slow-changing institutional data |
| Direct intake (SMS/email/form/API) | Real-time | Highest priority, process immediately |
| Tavily discovery searches | Every 6 hours | Catch new signals via targeted queries |

### Targeted Discovery Queries (Tavily)
Pointed searches per hotspot, rotated through categories and neighborhoods:

- "[City] volunteer opportunities this week"
- "[Neighborhood] food shelf donations needed"
- "[Region] mutual aid [year]"
- "[City] community events [month]"
- "[County] emergency assistance"
- "[City] GoFundMe medical"
- "[Neighborhood] cleanup help needed"
- Scaled per hotspot — more queries for higher-density areas

### System Architecture
```
┌─────────────────────────────────────────────────────┐
│                  Scheduler (cron)                     │
│       Triggers jobs per source × hotspot × cadence   │
└────────────────────┬────────────────────────────────┘
                     │
          ┌──────────▼──────────┐
          │   Scraper Workers    │
          │  (per-source logic)  │
          │  Firecrawl / Apify / │
          │  Tavily / APIs       │
          └──────────┬──────────┘
                     │ raw content
          ┌──────────▼──────────┐
          │  Extraction Worker   │
          │  (Claude API)        │
          │  Pass 1: Analytical  │
          │  Pass 2: Enrichment  │
          └──────────┬──────────┘
                     │ structured signals
          ┌──────────▼──────────┐
          │ Geo + Dedup + Store  │
          │ (PostgreSQL/pgvector)│
          └──────────┬──────────┘
                     │
          ┌──────────▼──────────┐
          │   Signal Database    │
          │   (the substrate)    │
          └──────────┬──────────┘
                     │
        ┌────────────┼────────────┐
        │            │            │
   ┌────▼────┐ ┌────▼────┐ ┌────▼────┐
   │ Read API│ │Write API│ │  Admin  │
   │(consume)│ │(produce)│ │(manage) │
   └────┬────┘ └────┬────┘ └─────────┘
        │            │
   ┌────▼────────────▼────┐
   │      Consumers        │
   │  - Community platforms │
   │  - mntogether.org     │
   │  - City dashboards    │
   │  - Crisis heat maps   │
   │  - Weekly digests     │
   │  - Healthcare tools   │
   │  - 3rd party apps     │
   └──────────────────────┘
```

---

## Expiration & Lifecycle

- **Events** → expire after event date
- **Fundraisers** → expire when goal reached or campaign ends (re-check source)
- **Volunteer opportunities** → expire based on posted end date, or after 30 days if no end date (re-scrape to verify)
- **Mutual aid requests** → expire after 14 days unless refreshed
- **Ongoing programs** → no expiration but freshness score decays; re-scraped periodically
- **Crisis signals** → elevated refresh rate during active crisis hotspots

Tier 2 enrichments accelerate lifecycle changes: social media signals indicating closure/completion trigger immediate re-verification.

---

## Hotspot Scaling Model

The service starts with one hotspot (Twin Cities metro) and scales by adding more:

**Local hotspots** — a city, a metro area, a county. Steady-state signal collection. This is the default mode.

**Crisis hotspots** — spun up in response to emergencies (natural disaster, humanitarian crisis). Higher scraping frequency, broader source net, urgency-weighted signal. Can be anywhere in the world. Can be temporary.

**Community-requested hotspots** — as the service opens up, communities can request or self-provision their own hotspot. They bring knowledge of local sources; the service provides the infrastructure.

**Ecological hotspots** — defined by ecological boundaries rather than political ones. A watershed, a coastline, a reef system, a migration corridor, the great garbage patch. These hotspots surface environmental stewardship signal — restoration projects, citizen science, pollution monitoring, wildlife rescue. Their boundaries are polygons, not circles.

Each hotspot has its own:
- Source configuration (which local outlets, orgs, and platforms to scrape)
- Discovery query templates (Tavily searches tuned to the area)
- Geographic boundary (center + radius, or polygon)
- Cadence tuning (crisis hotspots scrape more frequently)

---

## Cost Estimates (Single Hotspot — Twin Cities)

| Component | Service | Estimated Monthly Cost |
|-----------|---------|----------------------|
| Fly.io (app + workers) | Fly.io | $20-50 |
| PostgreSQL | Fly.io or Supabase | $15-30 |
| Claude API (extraction) | Anthropic | $50-150 |
| Firecrawl | Firecrawl | $20-50 |
| Apify (Tier 2 scrapers) | Apify | $50-100 |
| Tavily (discovery searches) | Tavily | $20-50 |
| Twilio (SMS/Voice, when enabled) | Twilio | $20-50 |
| Geocoding | Nominatim (free) or Google | $0-20 |
| **Total estimated** | | **$195-500/month** |

Costs scale roughly linearly per hotspot added, with some shared infrastructure savings.

---

## First Sprint — Minimum Viable Pipeline

Prove the signal quality. Nothing else matters until this works.

1. **GoFundMe scraper** — geo-filtered to Twin Cities, extract structured signal
2. **Tavily discovery** — 10-20 pointed queries for Twin Cities community needs
3. **5-10 known org websites** — scrape volunteer/donate/events pages via Firecrawl
4. **Eventbrite API** — pull public community events in Twin Cities metro
5. **Claude extraction** — Pass 1 only (analytical), produce structured signal records
6. **PostgreSQL storage** — `signals` + `signal_sources` + `hotspots` tables
7. **Simple output** — JSON feed or bare-bones HTML to visually inspect signal quality
8. **Assess** — Is the signal good? Is there enough volume? Would a person act on this?

Everything else layers on after this proves out.

---

## The Ecosystem

```
                    ┌─────────────────────┐
                    │       Taproot        │
                    │    (signal utility)  │
                    │  "What does this     │
                    │   community need?"   │
                    └──────────┬──────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
     ┌────────▼──────┐ ┌──────▼───────┐ ┌──────▼───────┐
     │   Lenses      │ │  Community   │ │  Third Party │
     │  - Explorer   │ │  Platforms   │ │  - City tools│
     │  - Heat map   │ │  - Forums    │ │  - Org apps  │
     │  - Digests    │ │  - Discord   │ │  - Bots      │
     │  - Referrals  │ │  - Circle    │ │  - Anything  │
     │  - Dashboards │ │  - Custom    │ │              │
     └───────────────┘ └──────────────┘ └──────────────┘
```

Taproot is the gravity well. Everything orbits around it.

The signal it serves spans all of life — people helping people, people stewarding land, people restoring ecosystems, people showing up for the living world. The pipeline doesn't distinguish. It finds where help is needed, and it makes that findable.
