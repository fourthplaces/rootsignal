---
date: 2026-02-15
topic: signal-root-unified-vision
---

# Signal Root: Unified Vision

## The Metaphor

Signal Root is modeled on **mycorrhizal networks** — the underground fungal networks that connect trees. When one tree detects pests, disease, or drought, it sends chemical signals through the root network so other trees can adapt before the threat reaches them.

This is the system we're building. **Collect facts. Make them findable. Let communities adapt.**

## What Is Signal?

A signal is **something someone broadcast into the world.** An ask, a give, an alert, a public record. Someone or some institution put information out into the ether — the system catches it and makes it findable.

The defining characteristic: **signal is a broadcast, not a description.**

| Signal (broadcast) | Not signal (description) |
|---|---|
| "We need food pantry volunteers" | "We hold services Sundays at 10am" |
| "Free meals Tuesdays for anyone" | "About us: founded in 1952" |
| "Know your rights workshop March 5" | "Our staff directory" |
| "Donations needed for flood relief" | "Our mission statement" |
| EPA publishes: factory fined $500K | Company's generic homepage |
| USAspending publishes: $50M ICE contract | Company's careers page |
| Court publishes: class action filed | Company's about page |

**Everything in the system is something someone put out into the world:**
- **Community broadcasts:** orgs asking for help, offering resources, alerting their community
- **Government broadcasts:** agencies publishing records, enforcement actions, contracts, filings
- **Legal broadcasts:** courts publishing filings, decisions, settlements

"Church on Sundays" is a description — nobody's asking for anything, offering anything, or documenting anything. "Church needs volunteers for food pantry" is a broadcast — someone is signaling a need into the network. That's the difference.

### Signal Structure: Semantic Graph

The LLM reads raw content — a social media post, a webpage, a database record — and parses it into structured meaning. Each signal becomes a node in a semantic graph:

```
"URGENT: We need food pantry volunteers this Saturday at Grace Church!"
                            ↓
                    LLM semantic parsing
                            ↓
{
  type: "ask",
  content: "We need food pantry volunteers this Saturday at Grace Church",
  entity: Grace Church,
  when: 2026-02-21,
  about: "volunteers",  ← derived from "we need volunteers" (schema.org: about)
  where: Grace Church   ← resolved to [lat, lng]
}
```

Every field is **derived semantically from the content** — the LLM reads what's there, the way a human would. "Food pantry" + "volunteers" = what's needed. "At Grace Church" = where. "This Saturday" = when.

**Urgency is not a system field.** The system stores facts — what, when, where, who. The user decides what's urgent to them. Emotional language ("URGENT!", "desperate") should not rank one signal above another. Temporal proximity (event is this Saturday) and scale ("3000 people need food") are facts already captured in `when` and `content`. The consumer app can sort by date proximity. The user reads the content and decides.

The signal node connects to the graph:

```
Signal (ask: "need food pantry volunteers")
  → entity: Grace Church
  → when: 2026-02-21
  → where: [lat, lng]

Grace Church (entity)
  → signals: [ask, give, give, event, ...]
  → location: [lat, lng]
  → related entities: [Diocese of MN (parent), ...]

Diocese of MN (entity)
  → signals: [informative: 990 filing, informative: federal grant, ...]
  → subsidiaries: [Grace Church, St. Mary's, ...]
```

**The raw content is still searchable as text** (full-text + pgvector embeddings). But the semantic structure enables:

- **Sort asks by date** — soonest first (temporal proximity is a fact, not a judgment)
- **Filter by location** — parsed from content, mapped to coordinates
- **Follow breadcrumbs** — signal → entity → related entities → their signals
- **Map view** — signals plotted geographically
- **Connect related signals** — river cleanup ask near factory with EPA violations (geographic proximity + semantic relatedness)

**No taxonomy, no tags, no curated categories.** The semantic structure is derived from the content itself. Users search in their own words. The system matches via full-text and embeddings. The structured fields (when, where) enable sorting and filtering without editorial categorization.

Signal types:

| Signal Type | What it means | Example |
|---|---|---|
| `ask` | Entity needs something. You can help. | "We need food pantry volunteers" |
| `give` | Entity offers something actionable. You can receive. | "Free meals Tuesdays" / "Food pantry Mon-Fri 9-5" |
| `event` | Something is happening. You can show up. | "Community meeting Thursday to discuss the development" |
| `informative` | A published fact. You can know. | "EPA fined factory $500K" / "$50M ICE contract awarded" |

**`give` means actionable** — someone can walk in and receive something. "Free meals Tuesday" is a give. "Food pantry Mon-Fri 9-5" is a give (the LLM infers this — a human reading it knows it means free food). "We advocate for clean water" is a statement, not a give — nobody can walk in and receive something.

**`event` captures movement** — people gathering to act. Community meetings, protests, workshops, cleanups. These are valid signal: the community is moving.

### Classification: Good Enough, Not Perfect

The LLM derives signal type semantically — the same way a human reading the content would classify it. This is interpretation (reading comprehension), not editorial judgment (value assessment). The LLM isn't saying "this matters" — it's saying "this is an ask / give / event / informative."

**The system will be wrong sometimes. That's fine.** Design for it:

- **95% accuracy is the bar, not 100%.** If 95% of asks show up as asks and 95% of gives show up as gives, the system works.
- **Misclassified content is still searchable.** A give that gets classified as informative doesn't disappear — it's still in the system, linked to the entity, findable via search. It just shows up in the wrong browse tab.
- **Users can flag misclassifications.** "This isn't right" → correction improves the system over time.
- **Systematic bias is the real risk, not individual errors.** Monitor classification patterns across languages, cultural contexts, and source types. If Spanish-language asks consistently get classified as informative, that's a problem to fix. A one-off misclassification is noise.

We should expect imperfection and design accordingly, not over-engineer for theoretical purity.

### Signal Sources

Social media is the ideal source for asks/gives. Institutional databases are the source for informative signals:

| Signal type | Best source | Why |
|---|---|---|
| `ask` | Social media, community posts | Explicit, timestamped, specific requests |
| `give` | Social media, community posts | Explicit, timestamped, specific offers |
| `informative` | Institutional databases (USAspending, EPA ECHO, etc.) | Structured, dated, government-published records |

Social media posts are inherently broadcasts — every post is someone putting something into the world. They're timestamped (solves staleness), specific ("we need 5 volunteers THIS Saturday"), and harder to game than static website text. The existing adapters (Instagram, Facebook, X, TikTok) already support this.

### The User Brings the Meaning

The system doesn't know what's important. The system collects broadcasts and makes them searchable. **The user brings the meaning.**

- Someone worried about ICE searches "ICE" → sees companies with ICE contracts → decides to stay elsewhere
- Someone who wants to help browses `ask` signals near them → searches "food" or "environment" in their own words
- Someone concerned about pollution searches a factory name → sees EPA violations in the content
- A journalist searches an entity → reads across all signal types

Same data, different meaning, depending on who's looking and what they search for.

### Discovery Is Solved

The user who wants to help but doesn't know where to start can browse `ask` signals near them: "Who's asking for help in my area?" Search in their own words to narrow: "food", "river cleanup", "legal help". The user who has resources to share can browse `give` signals: "What's already being offered nearby?" This isn't editorial — it's signal type + free-text search. No taxonomy needed.

## Signal Root vs. Consumer App

**Signal Root is the data layer.** It collects broadcasts, derives signal type semantically, links to entities, and makes everything searchable. It has no opinion about what matters.

**The consumer app is the presentation layer.** It designs around human needs: "Who needs help near me?" (asks), "What's available?" (gives), "What's happening?" (events), "What should I know about this company?" (informative). The app decides how to present signals. Signal Root just provides the data.

This separation matters because:
- Signal Root can serve multiple apps (admin dashboard, consumer app, journalist tools, API)
- Signal Root stays unbiased — presentation choices live in the app layer
- The app can evolve its UX without changing the underlying data model

## Unified Architecture

This isn't a new feature bolted onto the existing system. It's a reframing of what the entire system does.

**Old model:** Separate signal domains (human services, ecological stewardship, civic action) with an LLM qualification gate deciding what's "worth" monitoring.

**New model:** One pipeline. Signal Root is a **broadcast collector.**

```
Raw content (social post, webpage, database record)
    ↓
LLM parses into semantic graph:
  - Type: ask / give / event / informative
  - Content: natural language description
  - When: date/time (if present)
  - Where: location (if present, resolved to coordinates)
  - About: what's being asked/given/discussed (schema.org: about)
  - Entity: who broadcast it or who it's about
    ↓
Link to entity graph (entity relationships, parent/subsidiary, etc.)
    ↓
Normalize into existing polymorphic infra:
  - Location → locationables + locations (geocoded, map-ready)
  - Schedule → schedules (iCal-aligned: one-time, recurring, date ranges)
  - Embedding → embeddings (pgvector, HNSW index)
    ↓
Store with source citation
    ↓
Searchable via full-text + embeddings (pgvector)
Navigable via semantic graph (entity → signals → related entities)
```

The LLM step is **semantic parsing** — it reads content the way a human would and extracts structured meaning:

- "URGENT: We need food pantry volunteers this Saturday!" → `ask`, when: Saturday, about: volunteers
- "Food pantry Mon-Fri 9-5" → `give`, when: ongoing, about: food (inferred — a human knows what a food pantry is)
- "Community meeting Thursday to discuss the development" → `event`, when: Thursday, about: community discussion
- "River cleanup Saturday 9am, meet at Bridge Park" → `event`, when: Saturday 9am, where: Bridge Park
- USAspending row → `informative`, content: "GEO Group was awarded a $110M contract by ICE for custodial services, 2017-2022", when: 2017-2022

**This is what the existing extraction pipeline already does** — LLM reads content, outputs structured data. The pipeline doesn't change. The schema of what comes out of it expands.

The LLM will be wrong ~5% of the time. That's expected. Misclassified content is still searchable — it just shows up in the wrong browse tab. Users can flag errors. Systematic bias (e.g., consistent misclassification of Spanish-language content) is monitored and corrected.

The LLM qualification gate goes away. If a source produces signals, it's valuable. If it doesn't (just descriptions, about pages, schedules), adaptive cadence backs it off naturally. The data self-qualifies.

## Key Insight: Evidence, Not Opinion

Sentiment is useful as an initial detection mechanism (it tells you something is happening), but sentiment alone can be propaganda or exaggeration. Instead of scoring sentiment, we collect **institutional actions** — things that are objectively documented and verifiable.

An "issue" isn't something we define. Issues are emergent clusters of institutional activity. We don't say "ICE is bad" — we say "USAspending.gov shows a $50M contract between Company Y and ICE, awarded 2025-06-01."

## Two-Layer Architecture

### Layer 1: Broadcast Collection

Signals stored in the database. Each record has: **type + content + entity + source + timestamp.**

| Type | Content (LLM-derived, searchable) | Entity | Source |
|---|---|---|---|
| `ask` | "We need food pantry volunteers this Saturday" | Grace Church | Instagram post |
| `ask` | "Donations needed for flood relief" | Red Cross MN | Facebook post |
| `give` | "Free meals every Tuesday, no questions asked" | Community Kitchen | Community org post |
| `give` | "Food pantry open Mon-Fri 9-5, all welcome" | St. Mary's Church | Org website (static page) |
| `give` | "Free legal clinic for immigrants March 5" | ACLU Minnesota | Org website |
| `event` | "Community meeting Thursday to discuss the proposed development" | Neighborhood Assoc. | Facebook post |
| `event` | "River cleanup Saturday 9am, meet at Bridge Park" | Friends of the River | Instagram post |
| `event` | "Know your rights workshop March 5 at the library" | ACLU Minnesota | Org website |
| `informative` | "GEO Group was awarded a $110M contract by ICE for custodial services, 2017-2022" | GEO Group | USAspending.gov |
| `informative` | "EPA fined Company X $2M for Clean Water Act violation, high priority" | Company X | EPA ECHO |
| `informative` | "State AG filed lawsuit against Company X for consumer fraud" | Company X | CourtListener |
| `informative` | "OSHA cited Warehouse Corp for willful safety violation, $145K penalty" | Warehouse Corp | DOL enforcement data |

For community content (social posts, websites — including static pages), the LLM reads the content and infers the signal type the way a human would. "Food pantry Mon-Fri 9-5" becomes a `give` because a human knows that means free food. For institutional records (structured database rows), the LLM translates to natural language AND classifies the type. Both produce the same output: a searchable natural language signal with a type.

The content column is what users search. "Food" finds the food pantry ask and the food pantry give. "ICE" finds the ICE contract. "river cleanup" finds the event. No tags needed — the words are already there.

### Layer 2: Graph Connections & Breadcrumbs

The semantic graph connects signals to each other through entities, locations, and time. This layer enriches the graph:

- **Entity relationships** — parent/subsidiary, contractor/client, supplier (from OpenCorporates, SEC, LittleSis)
- **Geographic proximity** — signals near each other on the map (river cleanup ask + factory with EPA violations = same area)
- **Temporal clustering** — signals close in time about related entities
- **Cross-source linking** — same entity appears in USAspending + EPA ECHO + CourtListener

The graph enables **breadcrumb navigation:** users follow connections through the graph.

```
User sees: ask → "River cleanup Saturday at Bridge Park"
  → clicks entity: Friends of the River
    → sees their other signals (events, asks, gives)
    → sees nearby entities on map
      → clicks: Acme Factory (0.5 miles upstream)
        → sees: informative → "EPA fined Acme $500K, Clean Water Act, 2025"
        → sees: informative → "Acme holds $30M DoD contract"
        → sees: related entity → Acme is subsidiary of Parent Corp
          → clicks: Parent Corp
            → sees: informative → "Parent Corp lobbied on environmental deregulation"
```

The system doesn't editorialize this trail. It provides the links. The user follows the path they care about.

## Pressure Testing: Proactive Community Members

| User | Entry point | What the system shows | Outcome |
|---|---|---|---|
| "I want to help but don't know where to start" | Browse `ask` signals near me | Orgs broadcasting needs: volunteers, donations, supplies | Finds somewhere to show up |
| "I heard about ICE raids" | Search "ICE" | Companies with ICE contracts + legal aid orgs offering help | Informed action: boycott + find resources |
| "Is my employer ethical?" | Search company name | Entity page: contracts, violations, donations, filings | Makes their own judgment from facts |
| "I want to volunteer for environment" | Browse `ask` near me, search "river" or "cleanup" | River cleanups, conservation orgs asking for help | Finds opportunities |
| "Factory near my kid's school smells" | Search factory name | EPA ECHO violations, toxic release data, discharge permits | Armed with government records for school board meeting |
| "I want to boycott ICE supporters" | Search "ICE contract" | Entities with documented ICE contracts in their content | Changes spending habits based on facts |
| "What's going on in my zip code?" | Browse all signals by location | Asks, gives, informative records — filter by type | Explores what matters to them |
| "My neighbor got arrested by ICE" | Browse `give` near me, search "legal" or "immigration" | Legal aid clinics, know-your-rights workshops | Finds immediate resources |
| "I'm a journalist investigating" | Search entity name | Full broadcast history across all databases | Deep investigation with citations |
| "I run a nonprofit, who else works on homelessness?" | Browse `ask`/`give` near me, search "housing" or "shelter" | Other orgs broadcasting similar needs and offers | Finds potential collaborators |
| "Someone said Company X pollutes the river" | Search Company X | EPA records either confirm (violations exist) or don't | Claim verified or unsubstantiated |
| "I have extra food to donate" | Browse `ask` near me, search "food" | Orgs broadcasting that they need food donations | Matches supply to demand |

**All 12 scenarios work.** Three entry points, no taxonomy needed:
1. **Browse by signal type** (`ask` / `give` / `event` / `informative`) + location
2. **Search in your own words** (full-text + semantic embeddings)
3. **Search by entity** (company name, org name)

## How It Reframes the Existing System

The existing pipeline stays. What changes is what flows through it and how we think about qualification:

- **Sources** → crawl both institutional databases (USAspending, EPA ECHO, etc.) AND community sources (local orgs, news, social)
- **Extraction** → LLM derives signal type (`ask`/`give`/`informative`) semantically + translates structured records to natural language
- **Entities** → link ALL signals to entities. Entity relationships enable breadcrumb navigation.
- **Qualification** → removed. Adaptive cadence handles sources that produce nothing. No LLM gatekeeper.
- **Signal Root** (data layer) → stores signals with type + content + entity + source + timestamp. Serves via API.
- **Consumer app** (presentation layer) → designs around needs ("asks near you"), offers ("gives near you"), and entity investigation ("follow the breadcrumbs"). Separate from Signal Root.

## Seeding: Crawl Everything, Editorialize Nothing

The critical insight: **don't start from issues, start from data sources.** We don't decide "ICE is worth investigating." We crawl USAspending and every federal contract is there — DoD, NIH, EPA, ICE, USDA, all of it. ICE contracts aren't special. They're just rows in the same database.

| What we crawl | What we get | No editorial needed |
|---|---|---|
| USAspending (all agencies) | Every federal contract | ICE contracts are just contracts |
| EPA ECHO (all facilities) | Every violation | Polluters surface by having violations |
| DOL enforcement (all cases) | Every labor violation | Wage thieves surface by having cases |
| FEC (all contributions) | Every political donation | Patterns emerge from the data |
| PACER/CourtListener (all filings) | Every federal lawsuit | Litigation patterns are just facts |
| OFAC (full list) | Every sanctioned entity | Sanctions are just records |

Bias could creep in through which databases we choose to crawl. The answer: start with databases that have the best APIs and crawl broadly. USAspending alone covers all federal spending across every agency — about as unbiased a starting point as possible.

## Comprehensive Signal Catalog

### Federal Spending & Contracts
| Signal | Source | Example |
|---|---|---|
| Federal contract awarded | USAspending.gov API | "GEO Group awarded $110M contract by ICE, FY2025" |
| Subcontract relationship | USAspending sub-awards | "Catering Corp X subcontracts under ICE detention prime" |
| Entity debarred/suspended | SAM.gov Exclusions API | "Company X excluded from federal contracting" |
| Federal grant received | USAspending.gov | "Org Y received $2M DHS grant for surveillance tech" |
| Contractor misconduct record | POGO Contractor Misconduct DB | "Top contractor cited for fraud 3x in 5 years" |

### Environmental
| Signal | Source | Example |
|---|---|---|
| EPA violation cited | EPA ECHO API | "Facility fined $500K for Clean Water Act violation" |
| Toxic chemical release reported | Toxic Release Inventory | "Plant released 50,000 lbs of lead compounds into air" |
| Superfund site responsibility | EPA SEMS | "Company X named as Potentially Responsible Party" |
| Water discharge permit exceedance | NPDES via ECHO | "Facility exceeded permitted discharge limits 12x in 2025" |
| Drilling/extraction permit issued | BLM / Forest Service | "Drilling permit granted for boundary waters area" |

### Labor & Workplace
| Signal | Source | Example |
|---|---|---|
| OSHA violation (willful/serious) | OSHA enforcement data | "Willful safety violation, $145K penalty" |
| Wage theft investigation | DOL WHD data | "$2.3M in back wages owed to 340 workers" |
| Unfair labor practice charge | NLRB case data | "ULP charge filed: illegal termination during organizing" |
| Federal contractor civil rights violation | OFCCP via DOL | "Hiring discrimination found at federal contractor" |

### Corporate & Financial
| Signal | Source | Example |
|---|---|---|
| SEC enforcement action | SEC EDGAR | "Securities fraud complaint filed by SEC" |
| Beneficial ownership filing | SEC 13D/13G | "Private equity firm X acquired 30% stake in Company Y" |
| Executive compensation disclosed | IRS 990 / SEC proxy | "CEO compensation: $12M; median worker: $31K" |
| Corporate subsidiary relationship | OpenCorporates / SEC | "Detention Corp is subsidiary of Parent Holdings LLC" |
| Corporate penalty assessed | Violation Tracker (Good Jobs First) | "3 penalties totaling $4.2M across EPA, OSHA" |

### Political & Lobbying
| Signal | Source | Example |
|---|---|---|
| Campaign contribution | FEC API | "Company PAC donated $500K to Candidate X" |
| Lobbying activity on specific issue | Senate LDA disclosures | "Lobbied on immigration enforcement appropriations" |
| State-level campaign contribution | FollowTheMoney | "$200K to state legislators on energy committee" |
| Revolving door (govt → private) | OpenSecrets | "Former ICE director now VP at detention company" |

### Legal
| Signal | Source | Example |
|---|---|---|
| Federal lawsuit filed | PACER / CourtListener | "Class action: detained immigrants v. Company X" |
| State AG enforcement action | State AG press releases | "AG sues company for consumer fraud" |
| Consent decree entered | DOJ / Court records | "Company agrees to $50M environmental cleanup" |

### Financial Regulation
| Signal | Source | Example |
|---|---|---|
| Consumer complaints volume | CFPB Complaint DB | "1,200 complaints filed against Company X in 2025" |
| Bank enforcement action | FDIC / OCC | "Consent order issued for BSA/AML violations" |
| Discriminatory lending pattern | HMDA data | "Denial rate 3x higher for minority applicants" |

### Nonprofit & Tax
| Signal | Source | Example |
|---|---|---|
| 990 financial disclosure | IRS 990 via ProPublica API | "Nonprofit spent 8% on programs, 72% on salaries" |
| Tax-exempt status revoked | IRS auto-revocation list | "Organization lost 501(c)(3) status" |
| Foundation grant to entity | IRS 990-PF | "Foundation granted $1M to lobbying group" |

### Subsidies & Public Incentives
| Signal | Source | Example |
|---|---|---|
| Tax subsidy received | Subsidy Tracker (Good Jobs First) | "Company received $50M state tax break for HQ" |
| Failed job creation promise | Subsidy Tracker | "Promised 500 jobs, created 12" |

### International & Sanctions
| Signal | Source | Example |
|---|---|---|
| Entity sanctioned (OFAC SDN) | OFAC sanctions list | "Company placed on SDN list" |
| Export control restriction | BIS Entity List | "Added to entity list for human rights concerns" |

### Relationship/Network Signals (Layer 2 — Inferred)
| Signal | Source | Example |
|---|---|---|
| Parent-subsidiary link | OpenCorporates + SEC | "Hotel brand owned by conglomerate with ICE contracts" |
| Board interlock | LittleSis | "Board member sits on both Company X and PAC Y" |
| Supply chain connection | Contracts + corporate disclosures | "Enterprise provides fleet vehicles under ICE contract" |
| Investor/funder relationship | SEC 13F + 990-PF | "Major investor also funds anti-immigrant PAC" |

## Pressure Test & Resolutions

### Design Pattern: Credit Report, Not Scorecard

A credit report doesn't say "this person is bad with money." It shows accounts, balances, late payments, dates. You read it and draw your own conclusions. An entity's institutional activity report works the same way.

### Identified Risks & Resolutions

**Noise (millions of records, most mundane)**
- Resolution: Show actual records grouped by source database, sorted by date. A $3K GSA conference room and a $50M ICE detention contract are clearly different just by reading them. Users filter by what matters to them. Don't pre-filter.

**Entity matching (same company, different names across databases)**
- Resolution: Engineering problem, already solved by others. Good Jobs First's Violation Tracker does parent-subsidiary matching across 50+ agencies. OpenCorporates has 200M+ records with relationships. Start with exact-match on name + EIN/DUNS, add fuzzy matching over time.

**Context stripping (ICE contract for detention vs. office supplies)**
- Resolution: Source data already has context. USAspending includes product/service codes and descriptions. EPA ECHO classifies violations by statute and severity. OSHA distinguishes "willful" from "other-than-serious." Preserve what the source provides.

**Temporal staleness (showing expired contracts, resolved violations)**
- Resolution: Every record has a date and status. Show them. "Contract #ABC, $50M, ICE, 2017-2019, COMPLETED." Users can see it ended. Active records show as active.

**Completeness bias (public companies have more data than private ones)**
- Resolution: Be transparent. "This entity has 47 public records across 6 federal databases" vs "This entity has 2 public records across 1 database." Note when an entity is private and federal disclosure requirements are limited.

**False equivalence (minor paperwork violation vs. toxic dumping)**
- Resolution: Don't aggregate into counts. Show actual records with the severity classification the source database provides:
  - `EPA | Clean Water Act | High Priority Violation | Toxic discharge | $500K penalty | 2024-03-15`
  - `EPA | RCRA | Minor | Late paperwork filing | $0 penalty | 2024-06-01`

  These are obviously different when you read them. Counts hide that. Records reveal it.

**UI editorializing (presentation choices create emphasis)**
- Resolution: Organize by data source, not by narrative. Don't have a section called "Controversies." Have sections called "Federal Contracts (USAspending)", "Environmental Compliance (EPA ECHO)", "Labor (OSHA/DOL)", "Political Activity (FEC)." Structure follows source, not story. Chronological within each section, most recent first. No color coding, no severity badges, no aggregate scores.

**Weaponization (bad actors using data out of context)**
- Resolution: Every record links to its primary government source. You're an index. This is the same model LittleSis, OpenSecrets, and Good Jobs First operate under — established legal and ethical territory.

**Legal risk (aggregation creating new meaning)**
- Resolution: You are an index of public records with citations. You don't author claims. Every fact links to a government database. Same model ProPublica Nonprofit Explorer and Good Jobs First have operated under for years.

**The boring middle (99% of entities are unremarkable)**
- Resolution: Feature, not bug. Most entity pages being unremarkable means the remarkable ones stand out on their own merits, not because you highlighted them.

### Critical Design Constraint

**Resist the urge to build aggregate scores, rankings, or "worst offenders" lists.** The moment you do that, you become an editorial publication. Stay as an index of public records and the bias problem stays resolved.

## Bias Mitigation

The system has no opinion. Specifically:

- **No issue detection** — the system doesn't decide what's a problem. The user brings their concern.
- **No tension scoring** — the system doesn't rank what's important. Facts are facts.
- **No qualification gate** — no LLM decides what's "worth" monitoring. Adaptive cadence handles low-value sources mechanically.
- **No editorial UI** — organized by data source, not by narrative. No "controversies" section.
- **No aggregation** — no scores, no counts, no badges. Show actual records.
- **Broad crawling** — we crawl entire databases, not selected entities or topics.
- **Every fact has a citation** — links to the primary government or institutional source.
- **Completeness gaps disclosed** — we're transparent when data availability varies.

## Open Questions

- How do we model entity relationships (parent/subsidiary/supplier) in the existing schema?
- What's the crawl cadence for institutional data sources (many update quarterly/annually)?
- How do we handle evidence that gets retracted or corrected?
- What's the entity matching strategy? (EIN/DUNS exact match → fuzzy name matching → OpenCorporates/LittleSis enrichment?)
- How do we handle the volume of USAspending data? (Millions of records — do we ingest all or filter by entity relevance?)

## Next Steps

→ `/workflows:plan` for implementation details, starting with USAspending.gov API as the first data source (broadest, most unbiased starting point, covers ICE contracts alongside everything else)
