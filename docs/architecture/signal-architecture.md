# Signal Architecture

## The Metaphor

Signal Root is modeled on **mycorrhizal networks** — the underground fungal networks that connect trees. When one tree detects pests, disease, or drought, it sends chemical signals through the root network so other trees can adapt before the threat reaches them.

This is the system we're building. **Collect broadcasts. Make them findable. Let communities adapt.**

---

## What Is Signal

A signal is **something someone broadcast into the world.** An ask, a give, an alert, a public record. Someone or some institution put information out into the ether — the system catches it and makes it findable.

The defining characteristic: **signal is a broadcast, not a description.**

| Signal (broadcast) | Not signal (description) |
|---|---|
| "We need food pantry volunteers" | "We hold services Sundays at 10am" |
| "Free meals Tuesdays for anyone" | "About us: founded in 1952" |
| "Know your rights workshop March 5" | "Our staff directory" |
| EPA publishes: factory fined $500K | Company's generic homepage |
| USAspending: $50M ICE contract | Company's careers page |

### Four Signal Types

| Type | Meaning | Example |
|---|---|---|
| `ask` | Entity needs something. You can help. | "We need food pantry volunteers this Saturday" |
| `give` | Entity offers something actionable. You can receive. | "Free meals every Tuesday, no questions asked" |
| `event` | People are gathering. You can show up. | "Community meeting Thursday to discuss the development" |
| `informative` | A published fact. You can know. | "EPA fined Company X $2M for Clean Water Act violation" |

`give` means **actionable** — someone can walk in and receive something. "Food pantry Mon-Fri 9-5" is a give because the LLM reads it the way a human would: free food is available. "We advocate for clean water" is a statement, not a give.

`event` captures **movement** — people gathering to act. Community meetings, protests, workshops, cleanups.

`informative` captures **institutional records** — government database rows translated to natural language. Contracts, violations, enforcement actions, filings.

### Classification: Good Enough, Not Perfect

The LLM derives signal type semantically — the same way a human reading the content would classify it. This is interpretation (reading comprehension), not editorial judgment (value assessment).

- **95% accuracy is the bar, not 100%.** If 95% of asks show up as asks and 95% of gives show up as gives, the system works.
- **Misclassified content is still searchable.** A give that gets classified as informative doesn't disappear — it's still linked to the entity, findable via search. It just shows up in the wrong browse tab.
- **Users can flag misclassifications.** Corrections improve the system over time.
- **Systematic bias is the real risk, not individual errors.** Monitor classification patterns across languages, cultural contexts, and source types.

### What the System Does Not Store

- **No urgency field.** The system stores facts — what, when, where, who. The user decides what's urgent. Emotional language ("URGENT!", "desperate") does not rank one signal above another. Temporal proximity and scale are facts already captured in schedules and content.
- **No taxonomy, no tags, no curated categories.** The semantic structure is derived from the content itself. Users search in their own words. The system matches via full-text and embeddings.
- **No aggregate scores or rankings.** The moment you build "worst offenders" lists, you become an editorial publication. Stay as an index of public records.

---

## Three-Layer Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Layer 3: Activities (the "Why")                        │
│  Evidence-backed explanations for why signal clusters    │
│  exist. Adversarial validation. Explicit links only.    │
├─────────────────────────────────────────────────────────┤
│  Layer 2: Semantic Graph (Connections)                   │
│  Entity relationships, geographic proximity, temporal    │
│  clustering, cross-source linking. Breadcrumb nav.      │
├─────────────────────────────────────────────────────────┤
│  Layer 1: Broadcast Collection (the substrate)          │
│  Sources → Adapters → Scrape → Snapshot → LLM Extract   │
│  → Signals + Entities + Locations + Schedules            │
└─────────────────────────────────────────────────────────┘
```

### Layer 1: Broadcast Collection

Every signal enters the system through the same pipeline. The pipeline is domain-agnostic — it doesn't know whether it's extracting a volunteer opportunity or an EPA violation. Configuration (sources, prompts, adapters) determines what flows through it.

```
Source (URL)
  → Domain detection (parse_source_input)
    → Adapter selection (instagram, facebook, x, http, firecrawl, usaspending, epa_echo, ...)
      → Scrape → RawPage
        → page_snapshot (stored: url, content, html)
          → LLM signal extraction (config/prompts/signal_extraction.md)
            → ExtractedSignal structs
              → Signal rows + Entity links + Location + Schedule + Embedding
```

**Source creation is a single text input.** Admin pastes a URL. `parse_source_input()` detects the domain and selects the adapter. `https://instagram.com/gracechurch` → instagram adapter. `https://api.usaspending.gov/...` → usaspending adapter. No special forms, no source type dropdowns.

**No qualification gate.** Sources are created `is_active = TRUE` by default. If a source produces 0 signals after consecutive scrapes, adaptive cadence exponentially backs off the scraping frequency until it hits a ceiling. Low-value sources self-regulate mechanically. No LLM decides what's "worth" monitoring.

#### Signal Sources

| Source type | Best for | Why |
|---|---|---|
| Social media (Instagram, Facebook, X, TikTok) | `ask`, `give`, `event` | Explicit, timestamped, specific broadcasts. Every post is someone putting something into the world. |
| Organization websites | `ask`, `give`, `event` | Volunteer pages, donation needs, event listings. Scraped via Firecrawl/HTTP. |
| Institutional databases (USAspending, EPA ECHO) | `informative` | Structured, dated, government-published records. Entity-driven queries (not full crawl). |
| Search discovery (Tavily) | All types | Targeted searches per hotspot, rotated through neighborhoods and topics. |

#### Adapter Cadence

```
social     → base: 24h,  ceiling: 168h (7 days)
website    → base: 48h,  ceiling: 336h (14 days)
search     → base: 6h,   ceiling: 48h
institutional → base: 168h (weekly), ceiling: 720h (30 days)
```

Adaptive cadence: each consecutive scrape with 0 new signals doubles the interval. Each scrape that produces signals resets to base. Sources that produce nothing naturally fade to their ceiling without human intervention.

#### Institutional Adapters

Institutional APIs are just adapters — the same pattern as social media. The domain in the URL determines the adapter.

**USAspending** (`api.usaspending.gov`):
- Entity-driven queries: search by recipient name from source config
- Returns structured award JSON → LLM translates to natural language `informative` signal
- Example: `"GEO Group Inc was awarded a $110M contract by ICE for custodial services, 2017–2022"`
- No auth, 30 req/min rate limit with backoff

**EPA ECHO** (`echodata.epa.gov`):
- Two-step QID pattern: facility lookup → detailed report with violations
- Violation severity preserved in content (HPV, SNC, Serious Violator)
- Example: `"EPA fined Company X $2M for Clean Water Act violation, high priority"`

Both produce `Vec<RawPage>` — the same interface as every other adapter. The LLM reads the structured data and produces natural language signals, just as it reads a social media post and produces signals.

#### Deduplication

Signals are deduplicated via SHA-256 fingerprint over normalized key fields (type + content + entity + about). The `signals` table has a `UNIQUE(fingerprint, schema_version)` constraint. On conflict, the existing signal is updated (content, about, entity, confidence) rather than duplicated.

### Layer 2: Semantic Graph

The semantic graph connects signals to each other through entities, locations, and time. This is not a separate system — it's the natural result of the polymorphic infra that links signals to shared tables.

#### Entity Graph

Every signal links to an entity — the organization, company, or agency that broadcast it or that it's about. Entities connect to other entities via `entity_relationships`:

```
Signal (ask: "need food pantry volunteers")
  → entity: Grace Church
    → signals: [ask, give, give, event, ...]
    → related entities: [Diocese of MN (parent), ...]

Diocese of MN (entity)
  → signals: [informative: 990 filing, informative: federal grant, ...]
  → subsidiaries: [Grace Church, St. Mary's, ...]
```

Entity matching for institutional data uses a tiered strategy:
1. **Exact EIN/DUNS** → auto-link (confidence 1.0)
2. **Exact name** (normalized, case-insensitive) → auto-link (confidence 0.9)
3. **Fuzzy name** (Levenshtein < 3) → admin review queue (confidence 0.6)
4. **No match** → create new entity, queue for merge review

#### Breadcrumb Navigation

The graph enables users to follow connections:

```
User sees: ask → "River cleanup Saturday at Bridge Park"
  → clicks entity: Friends of the River
    → sees their other signals (events, asks, gives)
    → sees nearby entities on map
      → clicks: Acme Factory (0.5 miles upstream)
        → sees: informative → "EPA fined Acme $500K, Clean Water Act"
        → sees: informative → "Acme holds $30M DoD contract"
        → sees: related entity → subsidiary of Parent Corp
          → clicks: Parent Corp
            → sees: informative → "lobbied on environmental deregulation"
```

The system doesn't editorialize this trail. It provides the links. The user follows the path they care about.

#### Geographic Connections

Signals get locations via `locationables` (polymorphic: `locatable_type = 'signal'`). The LLM extracts raw location text during signal extraction; the extraction activity geocodes it into the `locations` table via `Location::find_or_create_from_extraction()`. Geo queries use the same Haversine joins as every other locatable resource.

Geographic proximity creates implicit connections — a river cleanup event near a factory with EPA violations shows up on the same map view without any explicit link between them.

#### Temporal Connections

Signals get temporal data via `schedules` (polymorphic: `scheduleable_type = 'signal'`). iCal-aligned: one-time events, recurring programs ("Food pantry Mon-Fri 9-5"), date ranges. Signal expiry is via `schedules.valid_through` — events expire on their date, asks/gives expire via `valid_through`, informative signals never expire (institutional records are permanent facts).

#### Search

Three entry points, no taxonomy needed:

1. **Browse by signal type** (`ask` / `give` / `event` / `informative`) + location
2. **Search in your own words** — full-text (`tsvector` on content + about, weighted A/B) + semantic embeddings (`pgvector` with HNSW index)
3. **Search by entity** — company name, org name → entity page → all their signals

### Layer 3: Activities (the "Why")

Signals tell you **what** is being broadcast. Activities tell you **why**.

An Activity is an evidence-backed explanation for why a cluster of signals exists. It's not an inference — every causal link must be grounded in explicit statements from the evidence.

**Status: Brainstorm phase.** This layer is designed but not yet implemented.

#### How Detection Works

The LLM is already reading content during signal extraction. Some content carries weight beyond the surface broadcast:
- Unusual language: "afraid to leave their homes"
- Causal framing: "because of recent enforcement actions"
- Emergency tone from an entity that doesn't normally signal urgency

When content hints at something deeper, the signal gets flagged for investigation.

#### Investigation: Three Evidence Paths

1. **Follow the link trail** — extract links from the triggering snapshot, LLM decides which are relevant, crawl and snapshot them (max 3 hops). Every page fetched becomes auditable evidence.
2. **Search for corroboration** — Tavily searches based on what the LLM is seeing ("ICE enforcement Twin Cities 2026"). Results get snapshotted.
3. **Social media already in DB** — query posts already captured by the scraping pipeline. Timestamped, attributable, unedited.

**Path 0 (always first):** Check if an existing Activity already matches via embedding similarity. If yes, link the new signal and stop. Primary cost control mechanism.

#### The Critical Constraint: Explicit Links Only

Every link from a signal to an Activity must be grounded in explicit statements from the evidence. The LLM does not infer causation — it finds where causation is already stated.

```
VALID:
  Church post says "helping families afraid to leave home due to immigration enforcement"
  → The church explicitly stated the cause. Link is grounded.

INVALID:
  Church says "rent relief available" (no stated reason)
  + LLM finds news about ICE in the same city
  → The church never said it's because of ICE. This is an assumption.
```

#### Adversarial Validation

Before an Activity is created, a second LLM pass pressure-tests it: Does the evidence support this? Is every link grounded in explicit statements? Are there simpler explanations? The investigator LLM finds a story; the validator LLM tries to break it. One extra LLM call — cheap insurance against confabulation.

#### Graph Convergence

The most powerful output is when multiple signals from different entities converge on the same Activity:

```
Signals (surface)              Activity (underlying)
─────────────────              ─────────────────────
rent relief (give)         ──→ ICE enforcement in Twin Cities
grocery delivery (give)    ──→ ICE enforcement in Twin Cities
know-your-rights (event)   ──→ ICE enforcement in Twin Cities
ride sharing (give)        ──→ ICE enforcement in Twin Cities
```

Activities become hubs with two sides:
- **Evidence signals** (left): why it's happening — news, government records, org statements
- **Response signals** (right): who's responding and what they need — asks, gives, events

#### Self-Expanding Awareness

When the investigation layer discovers relevant sources not yet in the system, it recommends them for addition. New sources start producing signals immediately; adaptive cadence handles low-value ones. The network grows toward where the signals are.

---

## Data Model

```
signals
├── id (uuid, PK)
├── signal_type (text: ask | give | event | informative)
├── content (text, full-text indexed)
├── about (text, full-text indexed)
├── entity_id (uuid → entities)
├── source_url (text)
├── page_snapshot_id (uuid → page_snapshots)
├── extraction_id (uuid → extractions)
├── institutional_source (text: usaspending, epa_echo, etc.)
├── institutional_record_id (text: external ID)
├── source_citation_url (text: link to government source)
├── confidence (real, default 0.7)
├── fingerprint (bytea, SHA-256 of normalized key fields)
├── schema_version (int, default 1)
├── in_language (text, BCP 47, default 'en')
├── search_vector (tsvector, generated: content 'A' + about 'B')
├── created_at (timestamptz)
└── updated_at (timestamptz)
    UNIQUE(fingerprint, schema_version)
```

**Polymorphic infra (no new tables — shared with listings, entities):**

```
locationables (locatable_type = 'signal', locatable_id = signal.id)
  → locations (latitude, longitude, city, state, postal_code)

schedules (scheduleable_type = 'signal', scheduleable_id = signal.id)
  → valid_from, valid_through, dtstart, repeat_frequency, byday

embeddings (embeddable_type = 'signal', embeddable_id = signal.id)
  → vector (pgvector, HNSW index)

signal_flags
  → signal_id, flag_type (wrong_type | wrong_entity | expired | spam),
    suggested_type, comment, resolved
```

### Entity Relationship Diagram

```
signals ──→ entities ──→ entity_relationships ──→ entities
  │              │
  ├──→ locationables ──→ locations
  ├──→ schedules
  ├──→ embeddings
  ├──→ signal_flags
  ├──→ page_snapshots ──→ extractions
  └──→ (source_url traces back to sources)
```

---

## Pipeline Detail

### 1. Source Creation

```
Admin enters URL: "https://instagram.com/gracechurch"
  → parse_source_input()
    → domain: instagram.com → source_type: "instagram"
    → name: derived from path ("gracechurch")
    → is_active: TRUE (no qualification gate)
    → cadence: base 24h (social category)
```

### 2. Scraping

```
Scheduler triggers scrape (based on cadence)
  → Adapter fetches content → Vec<RawPage>
    → Each RawPage → page_snapshot row (url, content, html)
      → extraction_status: 'pending'
```

### 3. Signal Extraction

```
extract_signals_from_snapshot(snapshot_id)
  → Load page_snapshot content
  → LLM call with signal_extraction prompt
    → Returns ExtractedSignals { signals: Vec<ExtractedSignal> }
  → For each ExtractedSignal:
    → Fingerprint (SHA-256 of type + content + entity + about)
    → Entity resolution (find_or_create by name)
    → Extraction record (provenance)
    → Signal row (upsert on fingerprint conflict)
    → Location normalization (city/state/zip → geocode → locationable)
    → Schedule creation (dates/recurrence → schedule row)
    → Embedding generation (content + about → pgvector)
```

### 4. Serving

```
GraphQL API:
  signals(type, entityId, search, lat, lng, radiusKm, since, limit, offset)
  signal(id)
  flagSignal(id, flagType, suggestedType, comment)

Signal type exposes:
  id, signalType, content, about, entity, locations, schedules,
  sourceCitationUrl, institutionalSource, confidence, inLanguage, createdAt
```

---

## Use Cases

### Community Members

| User intent | Entry point | What the system shows |
|---|---|---|
| "I want to help but don't know where to start" | Browse `ask` near me | Orgs broadcasting needs: volunteers, donations, supplies |
| "I want to volunteer for environment" | Browse `ask`, search "river" or "cleanup" | Cleanups, conservation orgs asking for help |
| "I have extra food to donate" | Browse `ask`, search "food" | Orgs that need food donations |
| "What's going on in my zip code?" | Browse all signals by location | Asks, gives, events, informative — filter by type |
| "My neighbor got arrested by ICE" | Browse `give`, search "legal" or "immigration" | Legal aid clinics, know-your-rights workshops |

### Investigators & Journalists

| User intent | Entry point | What the system shows |
|---|---|---|
| "Is my employer ethical?" | Search company name | Entity page: contracts, violations, filings |
| "Factory near my kid's school smells" | Search factory name | EPA ECHO violations, toxic release data |
| "I'm investigating a company" | Search entity name | Full broadcast history across all databases |
| "Someone said Company X pollutes" | Search Company X | EPA records either confirm or don't |

### Civic Action

| User intent | Entry point | What the system shows |
|---|---|---|
| "I heard about ICE raids" | Search "ICE" | Companies with ICE contracts + legal aid orgs |
| "I want to boycott ICE supporters" | Search "ICE contract" | Entities with documented ICE contracts |
| "Who else works on homelessness?" | Browse `ask`/`give`, search "housing" | Other orgs with similar needs and offers |

All scenarios work through three entry points: **browse by type + location**, **search in your own words**, **search by entity**.

---

## Key Design Decisions

| Decision | Choice | Why |
|---|---|---|
| Signal types | 4 types: ask, give, event, informative | Covers the full spectrum of broadcasts. Semantically derived by LLM, not editorially assigned. |
| No taxonomy/tags | Content is the metadata | Searchable via full-text + embeddings. No predefined categories to maintain or bias toward. |
| No urgency field | Dropped entirely | Emotional language shouldn't rank signals. Temporal proximity is a fact (in schedules). User decides. |
| No qualification gate | Adaptive cadence self-regulates | Sources that produce nothing back off exponentially. No LLM gatekeeper deciding what's "worth" monitoring. |
| Polymorphic infra | Reuse locationables, schedules, embeddings | No new geo/temporal tables. Signals get same Haversine queries, iCal schedules, map view as existing resources. |
| Entity-driven institutional queries | Query by recipient/facility name, not full crawl | Avoids 400M+ record ingestion. Eliminates bias of choosing which agencies to crawl. |
| Credit report, not scorecard | Show actual records, never aggregate | A $3K GSA conference room and a $50M ICE detention contract are obviously different when you read them. Counts hide that. Records reveal it. |
| Explicit links only (Activities) | Every causal link grounded in stated evidence | The system aggregates what sources explicitly say. It does not infer causation. |
| schema.org alignment | about, inLanguage, types map to Demand/Offer/Event/Report | Aligned with existing schema.org conventions. |
| Fingerprint dedup | SHA-256 of normalized key fields | Upsert on conflict — same signal from re-scrape updates rather than duplicates. |
| LLM accuracy target | 95%, not 100% | Design for imperfection. Misclassified content is still searchable. User flagging corrects over time. |

---

## Editorial Principles

The system has no opinion. Specifically:

- **No issue detection** — the system doesn't decide what's a problem. The user brings their concern.
- **No tension scoring** — the system doesn't rank what's important. Facts are facts.
- **No editorial UI** — organized by data source, not by narrative. No "controversies" section.
- **No aggregation** — no scores, no counts, no badges. Show actual records.
- **Broad crawling** — we crawl entire databases, not selected entities or topics.
- **Every fact has a citation** — links to the primary government or institutional source.
- **Completeness gaps disclosed** — transparent when data availability varies.

### The Inclusion Test

Every signal must pass three questions:

1. **Is it actionable?** Can a person do something constructive with this information?
2. **Is it affirmative?** Does it point toward something being built, offered, or organized?
3. **Does it build agency?** Does encountering this signal make someone feel more capable of participating?

**Excluded:** threat/surveillance data, crisis alerts (belong in emergency systems), partisan content, rumors, personal disputes, commercial advertising, health/safety data for its own sake.

**The pattern for edge cases:** when confronted with a problem, the system surfaces the organized, constructive response to that condition — not the condition itself. The response is the signal.

---

## Signal Root vs. Consumer App

**Signal Root is the data layer.** It collects broadcasts, derives signal type semantically, links to entities, and makes everything searchable. It has no opinion about what matters.

**The consumer app is the presentation layer.** It designs around human needs: "Who needs help near me?" (asks), "What's available?" (gives), "What's happening?" (events), "What should I know?" (informative).

This separation matters because:
- Signal Root can serve multiple apps (admin dashboard, consumer app, journalist tools, API)
- Signal Root stays unbiased — presentation choices live in the app layer
- The app can evolve its UX without changing the underlying data model

---

## Evolution

The signal architecture represents a fundamental reframing of the system:

**Before (listings model):**
- 30+ tag kinds, LLM qualification gate, signal domains (human services, ecological stewardship, civic action)
- Editorial decisions about what's "worth" monitoring
- Taxonomy rigidity — predefined categories couldn't cover the full spectrum
- No institutional data connected to community signals

**After (signal model):**
- 4 broadcast types, no taxonomy, no qualification gate
- Content is the metadata — searchable via full-text + embeddings
- Institutional databases feed `informative` signals through the same pipeline
- Adaptive cadence replaces editorial gatekeeping

**Future (Activities layer):**
- Signals explain what. Activities explain why.
- Evidence-backed, adversarially validated, explicitly linked
- Self-expanding awareness: the network grows toward where the signals are

---

## References

### Internal

| Document | Purpose |
|---|---|
| `docs/brainstorms/2026-02-15-institutional-accountability-signals-brainstorm.md` | The foundational brainstorm — signal types, use cases, pressure tests, bias mitigation |
| `docs/brainstorms/2026-02-15-why-layer-activities-brainstorm.md` | Activities layer design — detection, investigation, adversarial validation |
| `docs/plans/2026-02-15-feat-signal-root-unified-vision-plan.md` | Implementation plan — phases, migrations, acceptance criteria |
| `docs/vision/editorial-and-signal-inclusion-principles.md` | What we include and exclude, and why |
| `docs/vision/principles-and-values.md` | Core principles — signal as public good, utility not platform |
| `docs/vision/problem-space-positioning.md` | The problem we're solving and where Root Signal fits |
| `docs/architecture/signal-service-architecture.md` | Original service architecture (pre-signal reframing) |
| `docs/architecture/signal-taxonomy.md` | Original taxonomy (superseded by 4-type signal model) |
| `config/prompts/signal_extraction.md` | The LLM prompt for signal extraction |

### Code

| File | Purpose |
|---|---|
| `modules/rootsignal-core/src/types.rs` | `ExtractedSignal`, `ExtractedSignals`, `ResourceType::Signal` |
| `modules/rootsignal-domains/src/signals/models/signal.rs` | `Signal` struct, CRUD, search, geo queries |
| `modules/rootsignal-domains/src/signals/activities/extract_signals.rs` | LLM extraction → signal rows + polymorphic normalization |
| `modules/rootsignal-server/src/graphql/signals/` | GraphQL types, queries, mutations |
| `modules/rootsignal-domains/src/scraping/adapters/usaspending.rs` | USAspending API adapter |
| `modules/rootsignal-domains/src/scraping/adapters/epa_echo.rs` | EPA ECHO API adapter |
| `migrations/049_signals.sql` | Signals table + full-text search index |
| `migrations/051_signal_flags.sql` | User correction flags |

### External

| Resource | URL |
|---|---|
| USAspending API | `https://api.usaspending.gov/api/v2/` |
| EPA ECHO API | `https://echodata.epa.gov/echo/` |
