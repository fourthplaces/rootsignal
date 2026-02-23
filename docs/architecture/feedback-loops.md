# Feedback Loops

The scout is a self-tuning system. Its outputs become inputs to future runs through 21 feedback loops operating at different timescales. This document maps every loop — what data flows where, what behavior changes, and whether the loop amplifies or corrects.

---

## System Overview

How data flows through one scout cycle. Arrows show where outputs become inputs to future decisions.

```mermaid
flowchart TB
    subgraph SCRAPE ["Phase 1-4: Scrape & Extract"]
        Schedule["Scheduler<br/><i>weight → cadence</i>"]
        Scrape["Scrape URLs"]
        Hash{"Content<br/>changed?"}
        Extract["LLM Extract"]
        Dedup["3-Layer Dedup<br/><i>title → embedding → vector</i>"]
        Store["Store Signals"]

        Schedule --> Scrape --> Hash
        Hash -- yes --> Extract --> Dedup --> Store
        Hash -- no --> Refresh["Refresh timestamps"]
    end

    subgraph ENRICH ["Phase 5-6: Enrich"]
        Cluster["Clustering<br/><i>similarity → stories</i>"]
        Respond["Response Mapping<br/><i>Give ↔ Tension</i>"]
        Investigate["Investigation<br/><i>web search corroboration</i>"]
    end

    subgraph DISCOVER ["Phase 7: Discover"]
        Actors["Actor Emergence<br/><i>author + mentions → sources</i>"]
        Curiosity["Curiosity Engine<br/><i>LLM briefing → queries</i>"]
        Mechanical["Mechanical Fallback<br/><i>template queries</i>"]
    end

    subgraph GRAPH ["Graph (persistent state)"]
        Sources[("Sources<br/><i>weight, cadence,<br/>empty_runs</i>")]
        Signals[("Signals<br/><i>confidence,<br/>corroboration</i>")]
        Stories2[("Stories<br/><i>energy, velocity</i>")]
        ActorNodes[("Actors<br/><i>domains, signals</i>")]
        Evidence[("Evidence<br/><i>content_hash</i>")]
    end

    Store --> Signals
    Store --> Evidence
    Refresh --> Signals
    Cluster --> Stories2
    Respond --> Signals
    Investigate --> Evidence
    Actors --> Sources
    Curiosity --> Sources
    Mechanical --> Sources

    Sources --> Schedule
    Signals --> Cluster
    Signals --> Respond
    Signals --> Investigate
    Signals --> Curiosity
    Stories2 --> Curiosity
    ActorNodes --> Actors
    Evidence --> Hash
    Store -.->|"actor mentions"| ActorNodes

    Signals --> Dedup
    Sources -.->|"metrics"| Sources

    style SCRAPE fill:#1a1a2e,stroke:#e94560,color:#fff
    style ENRICH fill:#1a1a2e,stroke:#0f3460,color:#fff
    style DISCOVER fill:#1a1a2e,stroke:#16213e,color:#fff
    style GRAPH fill:#0f3460,stroke:#e94560,color:#fff
```

## Amplifying Loops

Productive activity begets more productive activity. These loops grow the graph.

```mermaid
flowchart LR
    subgraph L1 ["Loop 1: Source Weight"]
        S1_scrape["Scrape source"] --> S1_signals["Signals produced"]
        S1_signals --> S1_weight["Weight increases"]
        S1_weight --> S1_cadence["Cadence shortens"]
        S1_cadence --> S1_scrape
    end

    subgraph L3 ["Loop 3: Corroboration"]
        S3_signal["Signal exists"] --> S3_found["Same signal<br/>from different source"]
        S3_found --> S3_corrob["corroboration_count++<br/>source_diversity++"]
        S3_corrob --> S3_weight["Source weight<br/>diversity_factor boost"]
        S3_weight --> S3_signal
    end

    subgraph L16 ["Loop 16: Actor → Source"]
        S16_signal["Signal mentions<br/>organization"] --> S16_actor["Actor node<br/>created"]
        S16_actor --> S16_domain["Domains/URLs<br/>extracted"]
        S16_domain --> S16_source["New Source<br/>created"]
        S16_source --> S16_signal
    end

    style L1 fill:#1a1a2e,stroke:#e94560,color:#fff
    style L3 fill:#1a1a2e,stroke:#e94560,color:#fff
    style L16 fill:#1a1a2e,stroke:#e94560,color:#fff
```

## Corrective Loops

These loops counteract problems — removing stale data, rebalancing signal types, and degrading gracefully under budget pressure.

```mermaid
flowchart LR
    subgraph L2 ["Loop 2: Dead Source"]
        S2_scrape["Scrape yields<br/>0 signals"] --> S2_counter["empty_runs++"]
        S2_counter --> S2_check{"≥ 10?"}
        S2_check -- yes --> S2_deactivate["Source deactivated"]
        S2_check -- no --> S2_scrape
    end

    subgraph L4 ["Loop 4: Freshness Reaping"]
        S4_active["Signal exists"] --> S4_confirmed{"Re-confirmed<br/>by source?"}
        S4_confirmed -- yes --> S4_refresh["last_confirmed_active<br/>refreshed"]
        S4_refresh --> S4_active
        S4_confirmed -- no, 150d --> S4_delete["Signal reaped"]
    end

    subgraph L8 ["Loop 8: Type Imbalance"]
        S8_count["Count signal types"] --> S8_check{"Tensions >> Gives?"}
        S8_check -- yes --> S8_annotate["Briefing: 'Gives<br/>underrepresented'"]
        S8_annotate --> S8_queries["LLM generates<br/>resource queries"]
        S8_queries --> S8_gives["More Give<br/>signals found"]
        S8_gives --> S8_count
    end

    style L2 fill:#1a1a2e,stroke:#0f3460,color:#fff
    style L4 fill:#1a1a2e,stroke:#0f3460,color:#fff
    style L8 fill:#1a1a2e,stroke:#0f3460,color:#fff
```

## Curiosity Engine (Loop 5)

The most sophisticated loop. The LLM sees its own track record and adjusts strategy.

```mermaid
flowchart TB
    Build["Build briefing<br/>from graph"] --> Tensions["Unmet Tensions<br/><i>sorted: unmet first,<br/>then severity</i>"]
    Build --> Stories["Story Landscape<br/><i>by energy</i>"]
    Build --> Balance["Signal Balance<br/><i>type counts +<br/>imbalance annotations</i>"]
    Build --> Performance["Discovery Performance<br/><i>top 5 successes,<br/>bottom 5 failures</i>"]
    Build --> Existing["Existing Queries<br/><i>for dedup</i>"]

    Tensions --> Prompt["Formatted Briefing<br/>(structured text)"]
    Stories --> Prompt
    Balance --> Prompt
    Performance --> Prompt
    Existing --> Prompt

    Prompt --> LLM["Claude Haiku<br/><i>'Where should I look next?'</i>"]
    LLM --> Plan["DiscoveryPlan<br/><i>3-7 queries with reasoning</i>"]

    Plan --> Dedup{"Substring<br/>dedup"}
    Dedup -- novel --> Create["Create WebQuery<br/>SourceNode"]
    Dedup -- duplicate --> Skip["Skip"]

    Create --> Search["Future run:<br/>web search"]
    Search --> Signals["Signals produced<br/>(or not)"]

    Signals --> Metrics["Source metrics update:<br/>signals_produced, weight,<br/>consecutive_empty_runs"]
    Metrics --> Performance

    Create -.->|"gap_context stores<br/>LLM reasoning"| Performance

    subgraph Fallback ["Fallback Triggers"]
        F1["No API key"]
        F2["Budget exhausted"]
        F3["Cold start<br/>< 3 tensions"]
    end

    Fallback --> Mechanical["Mechanical discovery<br/><i>'{what_would_help}<br/>resources services {city}'</i>"]

    style Build fill:#16213e,stroke:#e94560,color:#fff
    style LLM fill:#e94560,stroke:#fff,color:#fff
    style Prompt fill:#0f3460,stroke:#e94560,color:#fff
    style Fallback fill:#1a1a2e,stroke:#0f3460,color:#fff
```

## Response Mapping Loop (Loops 6 + 9)

How discovered resources reduce tension priority over time.

```mermaid
flowchart LR
    T["Tension extracted<br/><i>severity=high,<br/>what_would_help='food shelf'</i>"]
    T --> Unmet["Marked UNMET<br/><i>no RESPONDS_TO edges</i>"]
    Unmet --> Brief["High priority in<br/>discovery briefing"]
    Brief --> Query["LLM generates:<br/>'food shelf locations<br/>Minneapolis'"]
    Query --> Source["New WebQuery<br/>source created"]
    Source --> Scrape["Scraped next run"]
    Scrape --> Give["Give signal found:<br/>'Second Harvest food shelf<br/>expansion program'"]
    Give --> Map["Response Mapper:<br/>cosine similarity > 0.7"]
    Map --> Edge["RESPONDS_TO edge<br/><i>match_strength=0.82</i>"]
    Edge --> Met["Tension now has<br/>response_count=1"]
    Met --> Lower["Lower priority<br/>in next briefing"]

    style T fill:#e94560,stroke:#fff,color:#fff
    style Give fill:#16213e,stroke:#e94560,color:#fff
    style Edge fill:#0f3460,stroke:#e94560,color:#fff
```

## Dedup Pipeline (Loops 13-15)

Three layers prevent the same information from appearing as multiple signals.

```mermaid
flowchart TB
    Content["Scraped content"] --> Hash{"Layer 1:<br/>Content Hash<br/><i>FNV-1a(content, url)</i>"}
    Hash -- "same hash + url" --> SkipExtract["Skip extraction<br/>refresh timestamps"]
    Hash -- "new content" --> Extract["LLM extraction"]

    Extract --> TitleDedup{"Layer 2:<br/>Title Dedup<br/><i>exact match by<br/>(title, type, url)</i>"}
    TitleDedup -- "match, same source" --> RefreshOnly["Refresh<br/><i>no corroboration</i>"]
    TitleDedup -- "match, diff source" --> Corroborate1["Corroborate"]
    TitleDedup -- "no match" --> Embed["Batch embed"]

    Embed --> CacheCheck{"Layer 3a:<br/>Embedding Cache<br/><i>in-memory, this run</i>"}
    CacheCheck -- "sim ≥ 0.85 same-src" --> RefreshOnly2["Refresh"]
    CacheCheck -- "sim ≥ 0.92 cross-src" --> Corroborate2["Corroborate"]
    CacheCheck -- "no match" --> GraphCheck{"Layer 3b:<br/>Vector Index<br/><i>graph, all runs</i>"}

    GraphCheck -- "sim ≥ 0.85 same-src" --> RefreshOnly3["Refresh"]
    GraphCheck -- "sim ≥ 0.92 cross-src" --> Corroborate3["Corroborate"]
    GraphCheck -- "no match" --> Create["Create new signal"]

    style Hash fill:#0f3460,stroke:#e94560,color:#fff
    style TitleDedup fill:#0f3460,stroke:#e94560,color:#fff
    style CacheCheck fill:#0f3460,stroke:#e94560,color:#fff
    style GraphCheck fill:#0f3460,stroke:#e94560,color:#fff
    style Create fill:#16213e,stroke:#e94560,color:#fff
```

## Budget Degradation (Loop 11)

The system gracefully drops expensive features as budget is consumed.

```mermaid
flowchart LR
    subgraph Full ["Full Budget"]
        direction TB
        P1["Scrape + Extract"]
        P2["Clustering"]
        P3["Response Mapping<br/><i>~10 Haiku calls</i>"]
        P4["Investigation<br/><i>Haiku + web search</i>"]
        P5["LLM Discovery<br/><i>1 Haiku call</i>"]
        P1 --> P2 --> P3 --> P4 --> P5
    end

    subgraph Low ["Budget Low"]
        direction TB
        Q1["Scrape + Extract"]
        Q2["Clustering"]
        Q3["Response Mapping<br/>⚠ SKIPPED"]
        Q4["Investigation<br/>⚠ SKIPPED"]
        Q5["Mechanical Discovery<br/><i>template fallback</i>"]
        Q1 --> Q2 --> Q3 --> Q4 --> Q5
    end

    subgraph Zero ["No Budget"]
        direction TB
        R1["Scrape + Extract"]
        R2["Clustering"]
        R3["Actor Emergence<br/><i>graph reads only</i>"]
        R1 --> R2 --> R3
    end

    Full --> Low --> Zero

    style Full fill:#16213e,stroke:#e94560,color:#fff
    style Low fill:#1a1a2e,stroke:#e94560,color:#fff
    style Zero fill:#0f3460,stroke:#e94560,color:#fff
```

## Source Lifecycle

How a source moves through the system from discovery to deactivation.

```mermaid
stateDiagram-v2
    [*] --> Discovered: Actor mention / LLM query / Topic search / Curated
    Discovered --> Active: weight=0.3, cadence=3d

    Active --> Productive: signals_produced > 0
    Productive --> HighValue: weight > 0.8
    HighValue --> Productive: weight drops

    Active --> Struggling: empty_runs increasing
    Struggling --> Active: produces signal (resets counter)
    Struggling --> Deactivated: 10 consecutive empties

    Productive --> Struggling: stops producing

    Deactivated --> [*]: removed from schedule

    note right of HighValue: cadence = 6h
    note right of Active: cadence = 24h-3d
    note right of Struggling: cadence = 3d-7d
    note left of Deactivated: non-curated only
```

---

## Loop Map

| # | Loop | Type | Timescale | Files |
|---|------|------|-----------|-------|
| 1 | [Source Weight & Scheduling](#1-source-weight--scheduling) | Reinforcing + Balancing | Across runs | scheduler.rs, scout.rs |
| 2 | [Dead Source Deactivation](#2-dead-source-deactivation) | Balancing | Across runs | writer.rs, scout.rs |
| 3 | [Corroboration](#3-corroboration) | Reinforcing | Within + across | scout.rs, writer.rs |
| 4 | [Freshness Reaping](#4-freshness-reaping) | Balancing | Across runs | writer.rs |
| 5 | [Discovery Briefing (Curiosity Engine)](#5-discovery-briefing-curiosity-engine) | Reinforcing | Across runs | discovery.rs, writer.rs |
| 6 | [Unmet Tensions → Discovery Priority](#6-unmet-tensions--discovery-priority) | Reinforcing | Within run | discovery.rs, writer.rs |
| 7 | [Story Energy & Velocity](#7-story-energy--velocity) | Reinforcing | Across runs | cluster.rs, writer.rs |
| 8 | [Signal Type Imbalance → Discovery](#8-signal-type-imbalance--discovery) | Balancing | Within run | discovery.rs, writer.rs |
| 9 | [Response Mapping (RESPONDS_TO)](#9-response-mapping-responds_to) | Reinforcing | Within run | response.rs, writer.rs |
| 10 | [Investigation Cooldown](#10-investigation-cooldown) | Balancing | Across runs | investigator.rs, writer.rs |
| 11 | [Budget Exhaustion → Phase Skipping](#11-budget-exhaustion--phase-skipping) | Balancing | Within run | budget.rs, scout.rs, discovery.rs |
| 12 | [Quality Penalty](#12-quality-penalty) | Balancing | Persistent | quality.rs, scout.rs |
| 13 | [Content Hash Dedup](#13-content-hash-dedup) | Balancing | Within + across | scout.rs, writer.rs |
| 14 | [Embedding Cache (Within-Batch)](#14-embedding-cache-within-batch) | Balancing | Within run | scout.rs |
| 15 | [Vector Dedup (Cross-Run)](#15-vector-dedup-cross-run) | Balancing | Across runs | scout.rs, writer.rs |
| 16 | [Actor Mentions → Source Discovery](#16-actor-mentions--source-discovery) | Reinforcing | Across runs | scout.rs, discovery.rs |
| 17 | [Topic Discovery → New Accounts](#17-topic-discovery--new-accounts) | Reinforcing | Within + across | scout.rs |
| 18 | [Mechanical Discovery Fallback](#18-mechanical-discovery-fallback) | Balancing | Within run | discovery.rs |
| 19 | [Geo-Filtering Confidence Penalty](#19-geo-filtering-confidence-penalty) | Balancing | Persistent | scout.rs |
| 20 | [Source Diversity Factor](#20-source-diversity-factor) | Informing | Persistent | writer.rs |
| 21 | [Actor Signal Count Tracking](#21-actor-signal-count-tracking) | Informing | Across runs | writer.rs |

---

## Detail

### 1. Source Weight & Scheduling

The central feedback loop. Every source has a weight (0.1–1.0) that determines how often it's scraped.

**Produces:** `signals_produced`, `signals_corroborated`, `consecutive_empty_runs`, `last_produced_signal` on each Source node.

**Consumes:** `compute_weight()` in scheduler.rs combines these into a single weight score:

```
weight = base_yield * tension_bonus * recency_factor * diversity_factor

base_yield    = Bayesian smoothed signal/scrape ratio (prior: 0.3)
tension_bonus = 1.0 + (tension_signals / total_signals), capped at 2.0
recency_factor= 1.0 (recent) → 0.5 (30+ days since last signal)
diversity_factor = 1.0 + (corroboration_ratio * 0.5), max 1.5x
```

**Effect:** Weight maps to cadence: >0.8 → 6h, 0.5–0.8 → 24h, 0.2–0.5 → 3d, <0.2 → 7d. Productive sources get scraped more. Unproductive ones fade to weekly.

---

### 2. Dead Source Deactivation

**Produces:** `consecutive_empty_runs` counter incremented on each scrape that yields zero signals.

**Consumes:** `deactivate_dead_sources(10)` marks sources with 10+ consecutive empties as `active=false`.

**Effect:** Non-curated sources that consistently produce nothing are removed from the schedule. Curated sources are immune — they represent editorial judgment.

---

### 3. Corroboration

**Produces:** When the same signal is found from a different source URL:
- `corroboration_count` incremented on the signal
- `source_diversity` recomputed (unique entity domains in evidence)
- `external_ratio` updated (fraction of evidence from non-self sources)
- New Evidence node created linking signal to corroborating source

**Consumes:** Source weight formula uses `diversity_factor` (loop 1). Story synthesis uses corroboration depth.

**Effect:** Signals confirmed by multiple independent sources rise in credibility. Sources that produce corroborated signals get weight boosts. Two thresholds: 0.85 cosine similarity for same-source (refresh only), 0.92 for cross-source (real corroboration).

---

### 4. Freshness Reaping

**Produces:** `last_confirmed_active` timestamp refreshed on every scrape — even unchanged content.

**Consumes:** `reap_expired()` runs at the start of each scout cycle and deletes:
- Past non-recurring events (end/start time + grace period)
- Asks older than 30 days
- Notices older than 30 days
- Gives/Tensions not confirmed in 150 days

**Effect:** The graph stays current. Signals that disappear from their sources eventually expire. Same-source re-scrapes keep signals alive without inflating corroboration.

---

### 5. Discovery Briefing (Curiosity Engine)

The system's learning loop. Past discovery results inform future discovery queries.

**Produces:** Each LLM-discovered source stores its reasoning in `gap_context`: `"Curiosity: {reasoning} | Gap: {gap_type} | Related: {tension}"`. Over time, sources accumulate `signals_produced`, `weight`, and `consecutive_empty_runs`.

**Consumes:** `build_briefing()` queries the graph for:
- Top 5 successful discoveries (active, signals_produced > 0, by weight)
- Bottom 5 failures (deactivated or 3+ empty runs)

These appear in the LLM prompt as "Worked well" and "Didn't work" sections, with the original reasoning visible so the LLM can diagnose *why* a query succeeded or failed.

**Effect:** The LLM avoids repeating failed patterns and doubles down on strategies that worked. The `gap_context` field preserves provenance — a query that failed because "youth mentorship" doesn't have web presence is different from one that failed because the query was too vague.

---

### 6. Unmet Tensions → Discovery Priority

**Produces:** Tension nodes with `severity` and `what_would_help` fields. `RESPONDS_TO` edges track which tensions have response resources.

**Consumes:** `get_unmet_tensions()` returns tensions ordered by: unmet first (no RESPONDS_TO edges), then by severity DESC. These appear as the highest-priority section in the discovery briefing.

**Effect:** As RESPONDS_TO edges form, unmet tensions shrink. Discovery naturally shifts from "find any resources" to "find resources for the remaining unmet tensions." Critical unmet tensions always surface first.

---

### 7. Story Energy & Velocity

**Produces:** `ClusterSnapshot` nodes created each clustering run, storing signal_count and entity_count at that timestamp.

**Consumes:** Velocity calculated as `(current_entity_count - entity_count_7d_ago) / 7`. Entity diversity (not raw signal count) drives velocity — prevents spam inflation.

**Effect:** High-energy stories appear in the discovery briefing's story landscape section. Rapidly growing stories signal emerging narratives that may warrant deeper investigation.

---

### 8. Signal Type Imbalance → Discovery

**Produces:** `get_signal_type_counts()` aggregates active signal counts per type (Event, Give, Ask, Notice, Tension).

**Consumes:** The briefing annotates significant imbalances:
- `tensions > 3 * gives` → "Give signals significantly underrepresented"
- `asks > 2 * gives` → "Few Give signals to match Ask signals"

**Effect:** When the graph has 31 tensions but only 8 gives, the LLM is nudged toward finding resources, programs, and services rather than more problem reports. A corrective force against the natural bias of news sources toward problem coverage.

---

### 9. Response Mapping (RESPONDS_TO)

**Produces:** `RESPONDS_TO` edges between Give/Event/Ask signals and Tension nodes, with `match_strength` and `explanation` properties.

**Consumes:** `get_unmet_tensions()` checks for incoming RESPONDS_TO edges. Tensions with zero responses are marked `unmet=true`.

**Effect:** Closes the loop between tension identification and resource discovery. When a Give signal (e.g., "food shelf expansion") is found that responds to a Tension ("Northside food desert"), the tension drops in discovery priority. The system tracks which problems have solutions.

---

### 10. Investigation Cooldown

**Produces:** `investigated_at` timestamp set on signal nodes after investigation.

**Consumes:** `find_investigation_targets()` filters out signals investigated within the last 7 days. Per-domain dedup ensures max 1 target per source domain.

**Effect:** Prevents wasting web search budget investigating the same signals repeatedly. The 7-day cooldown allows new evidence to accumulate before re-investigation. Domain dedup prevents one prolific source from consuming the entire investigation budget.

---

### 11. Budget Exhaustion → Phase Skipping

**Produces:** `spent_cents` atomic counter incremented by each operation.

**Consumes:** Phase guards check `has_budget()` before expensive operations:
- Response mapping: needs ~10x CLAUDE_HAIKU_SYNTHESIS
- Investigation: needs CLAUDE_HAIKU_INVESTIGATION + SEARCH_INVESTIGATION
- LLM discovery: needs CLAUDE_HAIKU_DISCOVERY

**Effect:** Four-level degradation:
1. Full LLM discovery (normal)
2. Mechanical template fallback (budget/API/cold-start)
3. Actor emergence only (always free — graph reads from extraction)
4. No discovery (no tensions/actors — correct behavior)

The system never crashes from budget exhaustion. It gracefully drops expensive features.

---

### 12. Quality Penalty

**Produces:** `quality_penalty` multiplier (1.0 = no penalty, <1.0 = penalized) set by quality scoring or supervisor override.

**Consumes:** Applied in weight calculation: `new_weight = (base_weight * quality_penalty).clamp(0.1, 1.0)`.

**Effect:** Sources producing low-quality signals (poor geo accuracy, missing action URLs, low confidence) get reduced scheduling priority. A supervisor can also manually penalize sources.

---

### 13. Content Hash Dedup

**Produces:** FNV-1a content hash stored in Evidence nodes (`content_hash`).

**Consumes:** `content_already_processed(hash, url)` checks before extraction. If content from the same URL hasn't changed, extraction is skipped entirely.

**Effect:** Saves LLM extraction budget on unchanged pages while still refreshing signal timestamps. Scoped to (hash, URL) so the same content from a different URL still gets processed (cross-source).

---

### 14. Embedding Cache (Within-Batch)

**Produces:** In-memory embedding vectors cached during a scout run.

**Consumes:** Before each graph vector search, the cache is checked first. Catches duplicates between Instagram and Facebook posts from the same org processed in the same batch.

**Effect:** Prevents duplicate signals within a single run without waiting for graph indexing. Same-source matches refresh only; cross-source matches at 0.92+ similarity trigger corroboration.

---

### 15. Vector Dedup (Cross-Run)

**Produces:** Embedding vectors stored on signal nodes and indexed for vector search.

**Consumes:** `find_duplicate()` queries graph vector indices (one per signal type) with 0.85 threshold.

**Effect:** Catches semantic duplicates across runs. A signal about "bike lane conflict on Hennepin" matches "bicycle infrastructure dispute Hennepin Ave" even with different wording. Same-source → refresh; cross-source >=0.92 → corroborate.

---

### 16. Actor Mentions → Source Discovery

**Produces:** Actor nodes created when mentioned in extracted signals. `ACTED_IN` edges link actors to signals.

**Consumes:** `discover_from_actors()` queries actors with domains/social_urls not yet tracked as sources.

**Effect:** Closes a discovery loop: signals mentioning organizations → actor nodes with domains → new source nodes → future scrapes → more signals. Organizations active in community life automatically become tracked sources.

---

### 17. Topic Discovery → New Accounts

**Produces:** New Instagram SourceNodes created when hashtag/topic search finds accounts posting signals.

**Consumes:** Next run's scheduling includes these new sources at starter weight (0.3).

**Effect:** Social media accounts discovered via topic searches get added to the source registry. If they produce signals, their weight increases (loop 1). If not, they get deactivated (loop 2).

---

### 18. Mechanical Discovery Fallback

**Produces:** Template-generated queries: `"{what_would_help} resources services {city}"`.

**Consumes:** Triggered when LLM discovery can't run (no API key, budget exhausted, cold start < 3 tensions).

**Effect:** Ensures the system always makes progress on discovery even without LLM access. Not as smart as the curiosity engine, but maintains forward momentum during outages or cold starts.

---

### 19. Geo-Filtering Confidence Penalty

**Produces:** Signals from city-local sources with unrecognized location names get `confidence *= 0.8`.

**Consumes:** Downstream ranking and filtering use confidence scores.

**Effect:** Signals that can't be geo-verified are kept but marked as less certain. Signals with coordinates outside the city radius are dropped entirely.

---

### 20. Source Diversity Factor

**Produces:** `source_diversity` (unique entity domains) and `external_ratio` computed from evidence nodes.

**Consumes:** Used in story-level quality metrics (corroboration_depth). Feeds into source weight diversity_factor (loop 1).

**Effect:** Signals backed by diverse, independent sources are treated as more reliable. Single-source signals remain but carry lower weight in story narratives.

---

### 21. Actor Signal Count Tracking

**Produces:** `Actor.signal_count` incremented on each mention. `Actor.last_active` timestamp updated.

**Consumes:** Enables "who's most active in community space" analysis. Actors with high signal counts and recent activity represent key community organizations.

**Effect:** Primarily informing — doesn't directly change scout behavior, but enriches the graph for downstream consumers (editions, API queries).

---

---

## Decision Trees

### Signal Ingestion

What happens when the scout encounters a piece of content.

```mermaid
flowchart TB
    Start["URL scraped"] --> Empty{"Content<br/>empty?"}
    Empty -- yes --> Fail["Mark failed"]
    Empty -- no --> HashCheck{"Hash + URL<br/>seen before?"}
    HashCheck -- yes --> Refresh["Refresh timestamps<br/>on existing signals"]
    HashCheck -- no --> Extract["LLM extraction<br/>(Haiku)"]
    Extract --> Nodes{"Signals<br/>extracted?"}
    Nodes -- "0 signals" --> RecordEmpty["Record empty scrape<br/>empty_runs++"]
    Nodes -- "≥1 signal" --> GeoCheck

    subgraph GeoCheck ["Geo Filter (per signal)"]
        G1{"Has<br/>coordinates?"}
        G1 -- yes --> G2{"Within city<br/>radius?"}
        G2 -- yes --> Accept1["Accept"]
        G2 -- no --> Drop["Drop signal"]
        G1 -- no --> G3{"location_name<br/>matches geo_term?"}
        G3 -- yes --> Accept2["Accept"]
        G3 -- no --> G4{"Source is<br/>city-local?"}
        G4 -- yes --> Penalize["Accept<br/>confidence × 0.8"]
        G4 -- no --> Drop2["Drop signal"]
    end

    Accept1 --> DedupTree
    Accept2 --> DedupTree
    Penalize --> DedupTree

    subgraph DedupTree ["Dedup Decision"]
        D0["Normalize title"] --> D1{"Same title+type<br/>from same URL?"}
        D1 -- yes --> D1a["Refresh only"]
        D1 -- no --> D2{"Same title+type<br/>from different URL?"}
        D2 -- yes --> D2a["Corroborate<br/>+ create evidence"]
        D2 -- no --> D3["Embed signal"]
        D3 --> D4{"Cache/vector<br/>sim ≥ 0.85?"}
        D4 -- "yes, same source" --> D4a["Refresh only"]
        D4 -- "yes, diff source<br/>≥ 0.92" --> D4b["Corroborate"]
        D4 -- "no match or<br/>< 0.92 cross-src" --> D5["CREATE new signal"]
    end

    style Start fill:#16213e,stroke:#e94560,color:#fff
    style D5 fill:#e94560,stroke:#fff,color:#fff
    style Drop fill:#555,stroke:#888,color:#ccc
    style Drop2 fill:#555,stroke:#888,color:#ccc
```

### Discovery Method Selection

How the scout decides which discovery strategy to use.

```mermaid
flowchart TB
    Start["Phase 7: Discovery"] --> Actors["Always: actor emergence<br/><i>author_actor + mentioned_actors<br/>from extraction</i>"]
    Actors --> HasClaude{"Claude API<br/>key set?"}
    HasClaude -- no --> Mechanical["Mechanical fallback<br/><i>template queries</i>"]
    HasClaude -- yes --> HasBudget{"Budget<br/>remaining?"}
    HasBudget -- no --> Mechanical
    HasBudget -- yes --> BuildBrief["Build briefing<br/>from graph"]
    BuildBrief --> BriefOk{"Briefing<br/>built OK?"}
    BriefOk -- error --> Mechanical
    BriefOk -- ok --> ColdStart{"Cold start?<br/><i>< 3 tensions<br/>AND 0 stories</i>"}
    ColdStart -- yes --> Mechanical
    ColdStart -- no --> LLMCall["Claude Haiku<br/>extract DiscoveryPlan"]
    LLMCall --> LLMOk{"LLM call<br/>succeeded?"}
    LLMOk -- error --> Mechanical
    LLMOk -- ok --> CreateSources["Create WebQuery<br/>sources (max 7)<br/><i>with dedup</i>"]

    Mechanical --> CreateMech["Create template queries<br/>(max 5)<br/><i>'{help} resources services {city}'</i>"]

    style Start fill:#16213e,stroke:#e94560,color:#fff
    style LLMCall fill:#e94560,stroke:#fff,color:#fff
    style Mechanical fill:#0f3460,stroke:#e94560,color:#fff
    style CreateSources fill:#16213e,stroke:#e94560,color:#fff
    style CreateMech fill:#0f3460,stroke:#e94560,color:#fff
```

### Source Scheduling

How the scheduler decides which sources to scrape this run.

```mermaid
flowchart TB
    Start["All active sources"] --> ForEach["For each source"]
    ForEach --> Cadence{"Time since<br/>last_scraped ≥<br/>cadence_hours?"}
    Cadence -- no --> Skip["Skip this run"]
    Cadence -- yes --> Scheduled["Add to schedule"]

    ForEach --> Explore{"Exploration<br/>policy?"}
    Explore -- "10% random<br/>from skipped" --> ExploreAdd["Add as exploration<br/><i>prevents weight death spiral</i>"]

    Scheduled --> WeightSort["Sort by weight DESC"]
    ExploreAdd --> WeightSort

    WeightSort --> Execute["Scrape in order"]

    subgraph CadenceTable ["Weight → Cadence Mapping"]
        W1["> 0.8 → 6h"]
        W2["0.5-0.8 → 24h"]
        W3["0.2-0.5 → 3 days"]
        W4["< 0.2 → 7 days"]
    end

    style Start fill:#16213e,stroke:#e94560,color:#fff
    style CadenceTable fill:#0f3460,stroke:#e94560,color:#fff
```

### Investigation Target Selection

How the investigator picks which signals to verify.

```mermaid
flowchart TB
    Start["Find targets"] --> P1["Priority 1:<br/>New tensions (< 24h)<br/>with < 2 evidence nodes"]
    P1 --> P2["Priority 2:<br/>High-urgency asks<br/>with < 2 evidence nodes"]
    P2 --> P3["Priority 3:<br/>Thin-story signals<br/>(emerging stories)"]

    P3 --> Filters["Apply filters"]

    subgraph Filters ["Filtering"]
        F1{"investigated_at<br/>> 7 days ago<br/>or null?"}
        F1 -- no --> FSkip["Skip<br/><i>cooldown active</i>"]
        F1 -- yes --> F2{"Domain already<br/>seen this run?"}
        F2 -- yes --> FSkip2["Skip<br/><i>per-domain dedup</i>"]
        F2 -- no --> FAccept["Accept target"]
    end

    FAccept --> Cap{"≤ 5 targets<br/>total?"}
    Cap -- yes --> Investigate
    Cap -- no --> Done["Stop"]

    subgraph Investigate ["Per Target"]
        I1["LLM generates<br/>1-3 search queries"]
        I1 --> I2["Web searches<br/><i>filter out same-domain</i>"]
        I2 --> I3["LLM evaluates<br/>evidence quality"]
        I3 --> I4{"confidence<br/>≥ 0.5?"}
        I4 -- yes --> I5["Create Evidence node"]
        I4 -- no --> I6["Discard"]
    end

    Cap -- yes --> I1
    I5 --> Mark["Mark signal<br/>investigated_at = now"]
    I6 --> Mark

    style Start fill:#16213e,stroke:#e94560,color:#fff
    style Investigate fill:#0f3460,stroke:#e94560,color:#fff
```

### Weight Computation

How source weight is calculated from accumulated metrics.

```mermaid
flowchart TB
    Inputs["Source metrics:<br/>signals_produced<br/>signals_corroborated<br/>total_scrapes<br/>tension_count<br/>last_produced_signal"]

    Inputs --> Yield["base_yield =<br/>(signals + 1) / (scrapes + 3)<br/><i>Bayesian smoothing,<br/>prior = 0.3</i>"]

    Inputs --> Tension["tension_bonus =<br/>1.0 + (tension_count / signals)<br/><i>capped at 2.0</i>"]

    Inputs --> Recency{"Days since<br/>last signal?"}
    Recency -- "< 14d" --> R1["recency = 1.0"]
    Recency -- "14-30d" --> R2["recency = 0.75"]
    Recency -- "30-90d" --> R3["recency = 0.5"]
    Recency -- "> 90d" --> R4["recency = 0.25"]

    Inputs --> Diversity["diversity_factor =<br/>1.0 + (corroboration_ratio × 0.5)<br/><i>max 1.5</i>"]

    Yield --> Combine
    Tension --> Combine
    R1 --> Combine
    R2 --> Combine
    R3 --> Combine
    R4 --> Combine
    Diversity --> Combine

    Combine["weight = yield × tension ×<br/>recency × diversity"]
    Combine --> QualityPenalty["× quality_penalty<br/><i>supervisor override</i>"]
    QualityPenalty --> Clamp["clamp(0.1, 1.0)"]
    Clamp --> Final["Final weight"]

    style Inputs fill:#16213e,stroke:#e94560,color:#fff
    style Combine fill:#e94560,stroke:#fff,color:#fff
    style Final fill:#0f3460,stroke:#e94560,color:#fff
```

---

## System Dynamics

The loops form three natural groups:

**Amplifying loops (signal → more signal):** Loops 1, 3, 5, 6, 9, 16, 17 — productive sources get more attention, corroborated signals get weight boosts, unmet tensions drive discovery.

**Corrective loops (poor signal → less signal):** Loops 2, 4, 8, 10, 11, 12, 18 — dead sources deactivated, stale signals reaped, imbalances corrected, budget forces graceful degradation.

**Dedup loops (prevent noise):** Loops 13, 14, 15 — content hashing, embedding cache, and vector search prevent the same information from appearing as multiple signals.

The curiosity engine (loop 5) is the most sophisticated — it's the only loop where the system explicitly reasons about its own past performance and adjusts strategy accordingly.
