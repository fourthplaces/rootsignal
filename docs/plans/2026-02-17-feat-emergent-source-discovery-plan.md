---
title: "feat: Emergent Source Discovery — Self-Expanding Signal"
type: feat
date: 2026-02-17
milestone: "2.5"
---

# Emergent Source Discovery — Self-Expanding Signal

## Overview

The source list becomes an output of the system, not an input. After each scout run, an LLM analyzes the signal graph to identify blind spots — orgs mentioned but not tracked, audience roles underrepresented, stories with thin corroboration — then generates search queries and candidate URLs to fill those gaps. Sources that produce corroborated, actionable signal earn trust. Sources that produce noise naturally decay. The system grows its own sensory apparatus.

## Problem Statement

Today all sources are hardcoded as `&'static str` in `sources.rs` CityProfile structs. To add a source, you edit Rust code and recompile. This creates three problems:

1. **Source selection bias** — whoever curates the list embeds their worldview. Somali community orgs, Hmong mutual aid, Spanish-language services may be invisible simply because no human thought to add them.
2. **Static coverage** — the civic landscape changes. New orgs form, old ones close, crises emerge. The source list doesn't adapt.
3. **Scaling bottleneck** — expanding to new cities requires manual source research per city. The Twin Cities profile has ~60 curated URLs + ~25 social accounts + 30 web search queries. Replicating this for every city doesn't scale.

The system already has the building blocks: Serper web search, scrape pipeline, embedding dedup, corroboration tracking, org diversity metrics. What's missing is the feedback loop that uses *what the system already knows* to discover *what it doesn't know yet*.

## Proposed Solution

Add a **source discovery phase** after clustering in the scout pipeline. This phase:

1. Reads the current signal graph landscape (stories, signal types, audience roles, org diversity, source domains)
2. Feeds a structured summary to an LLM that identifies gaps and generates search queries + candidate URLs
3. Creates `Source` nodes in the graph for discovered URLs
4. On the *next* scout run, discovered sources are loaded from the graph alongside the compiled seed sources
5. Per-source metrics (signals produced, corroboration rate) drive trust adjustment over time

```
Scout Run N:
  seed sources + discovered sources → scrape → extract → dedup → store → cluster → gap analysis → new Source nodes

Scout Run N+1:
  seed sources + discovered sources (now including N's discoveries) → scrape → extract → ...
```

## Technical Approach

### Source Node Schema

New graph label `Source` with the following struct in `types.rs`:

```rust
// modules/rootsignal-common/src/types.rs

pub struct SourceNode {
    pub id: Uuid,
    pub url: String,
    pub source_type: SourceType,          // "web", "instagram", "facebook", "reddit"
    pub discovery_method: DiscoveryMethod, // "curated", "gap_analysis", "signal_reference"
    pub city: String,                      // city key ("twincities", "nyc", etc.)
    pub trust: f32,                        // dynamic trust score, 0.0-1.0
    pub initial_trust: f32,                // TLD-based baseline from source_trust()
    pub created_at: DateTime<Utc>,
    pub last_scraped: Option<DateTime<Utc>>,
    pub last_produced_signal: Option<DateTime<Utc>>,
    pub signals_produced: u32,
    pub signals_corroborated: u32,
    pub consecutive_empty_runs: u32,       // for adaptive cadence
    pub active: bool,                      // false = skip scraping, retain history
    pub gap_context: Option<String>,       // why was this discovered? e.g. "fills senior audience gap"
}

pub enum SourceType {
    Web,
    Instagram,
    Facebook,
    Reddit,
}

pub enum DiscoveryMethod {
    Curated,       // from CityProfile seed
    GapAnalysis,   // LLM identified a gap and suggested this
    SignalReference, // extracted from signal content (org mentioned but not tracked)
}
```

**Edges:**
- `PRODUCED` — Source → Signal node (lightweight, for metrics aggregation)
- No edge to Evidence (Evidence already has `source_url` as a property, which is sufficient)

### Migration

In `migrate.rs`, add:

```rust
// Source node constraints and indexes
"CREATE CONSTRAINT ON (s:Source) ASSERT s.id IS UNIQUE"
"CREATE CONSTRAINT ON (s:Source) ASSERT s.url IS UNIQUE"
"CREATE INDEX ON :Source(city)"
"CREATE INDEX ON :Source(trust)"
"CREATE INDEX ON :Source(active)"
```

### Gap Analysis Phase

New module `modules/rootsignal-graph/src/discover.rs`:

```rust
pub struct SourceDiscoverer {
    client: GraphClient,
    writer: GraphWriter,
    anthropic_api_key: String,
    city: String,
}
```

**Input assembly** — query the graph for a landscape summary:

```rust
struct SignalLandscape {
    total_signals: u32,
    by_type: HashMap<String, u32>,           // Event: 30, Give: 87, Ask: 21, Notice: 9
    by_audience_role: HashMap<String, u32>,   // neighbor: 108, volunteer: 66, ...
    stories: Vec<StorySummary>,               // headline, signal_count, org_count, status
    source_domains: Vec<(String, u32)>,       // domain, signal_count
    org_distribution: Vec<(String, u32)>,     // org_id, signal_count
    mentioned_orgs_not_tracked: Vec<String>,  // orgs mentioned in signal text but not source list
    geographic_coverage: GeoCoverage,         // bounding box, signal density by quadrant
}
```

**LLM prompt** — structured gap analysis:

```
You are analyzing a civic signal graph for {city_name} to identify blind spots.

Current signal landscape:
{landscape_json}

Existing source domains: {domain_list}

Identify gaps in coverage. For each gap, suggest specific search queries OR URLs.

Rules:
- Focus on organizations, programs, and communities MENTIONED in existing signals but not currently tracked as sources
- Identify audience roles with thin coverage relative to the city's demographics
- Identify story clusters with < 2 distinct source orgs (thin corroboration)
- Do NOT generate queries designed to identify specific individuals
- Do NOT suggest sources behind login walls or private groups
- Prioritize .org, .gov, and established community organizations

Respond in JSON:
{
  "gaps": [
    {
      "description": "No direct tracking of Somali community organizations despite 4 signals mentioning them",
      "gap_type": "missing_org",  // missing_org | thin_corroboration | audience_gap | geographic_gap | type_gap
      "suggested_queries": ["Minneapolis Somali community organization services 2026"],
      "suggested_urls": ["https://www.somaliamerican.org/programs"],
      "priority": "high"  // high | medium | low
    }
  ]
}
```

**Output schema:**

```rust
#[derive(Deserialize, JsonSchema)]
struct GapAnalysisResponse {
    gaps: Vec<Gap>,
}

#[derive(Deserialize, JsonSchema)]
struct Gap {
    description: String,
    gap_type: String,
    suggested_queries: Vec<String>,
    suggested_urls: Vec<String>,
    priority: String,
}
```

**Model:** Haiku 4.5 (cost control). The landscape summary is structured data, not free-form reasoning — Haiku handles this well. Upgrade to Sonnet if gap quality is poor.

**Budget per run:** Max 5 new sources discovered per run. Max 3 new web search queries per run.

### Source Loading at Run Start

In `scout.rs`, augment `run_inner()` to load discovered sources from the graph:

```rust
// After loading CityProfile seed sources:
let discovered_sources = self.writer.get_active_sources(&self.profile.name).await?;
for source in discovered_sources {
    match source.source_type {
        SourceType::Web => all_urls.push((source.url, source.trust)),
        SourceType::Instagram => { /* add to instagram scrape list */ },
        SourceType::Facebook => { /* add to facebook scrape list */ },
        SourceType::Reddit => { /* add to reddit scrape list */ },
    }
}
```

### Trust Lifecycle — Evidence-Based Trust

Trust is not a formula. Trust is evidence.

The system already has `EvidenceNode` as a first-class concept — a struct with `source_url`, `content_hash`, `retrieved_at`, `snippet`. Evidence nodes are linked to signals via `SOURCED_FROM` edges. The insight: **trust is the density of evidence connected to a source's signals.**

**Initial trust:** `source_trust()` TLD-based heuristic (same as today). This is the system's prior — used for confidence-weighting before any evidence accumulates.

**Evidence accumulation:** When the Investigation loop (Phase 2) examines a signal or tension, it follows evidence chains — checking public records, 501(c)(3) databases, media archives, government grant listings. Each verified fact becomes an Evidence node in the graph. Evidence nodes connect to the signals they support.

**Trust as evidence density:**

```rust
fn compute_trust(source: &SourceNode, evidence_count: u32) -> f32 {
    if source.signals_produced == 0 {
        return source.initial_trust; // No track record, use TLD baseline
    }

    let evidence_density = evidence_count as f32 / source.signals_produced as f32;

    // Blend: evidence density anchored by TLD baseline
    // A source with rich evidence converges toward 0.95
    // A source with no evidence stays near its TLD baseline
    let evidence_weight = (source.signals_produced as f32 / 10.0).min(1.0); // ramp up over first 10 signals
    (evidence_weight * evidence_density.min(1.0) + (1.0 - evidence_weight) * source.initial_trust)
        .clamp(0.05, 0.95)
}
```

This dissolves the corroboration paradox: a niche Hmong mutual aid org's events page might never get corroborated by another source, but when the Investigator checks and finds a real 501(c)(3) registration, a real physical address, real program history — those Evidence nodes accumulate. The source earns trust through *depth of evidence*, not breadth of corroboration.

**Trust floor: 0.05** — per the principle "Root Signal will not gatekeep what enters the graph." Trust affects surfacing priority, never admission. A 0.05-trust source's signals still enter the graph, they just rank low in confidence-weighted queries.

**Deactivation:** Source is marked `active: false` if:
- `consecutive_empty_runs >= 10` (source consistently produces nothing — likely dead)
- Manual opt-out (durable — see Blocked Sources below)

Note: there is no trust-based deactivation. Low-evidence sources stay active but rank low. The system doesn't gatekeep — it just surfaces what has evidence behind it.

Deactivated sources remain in the graph for audit trail. Their existing signals are unaffected.

**Adaptive cadence (future enhancement):** Sources with `consecutive_empty_runs > 3` could be scraped every 2nd or 3rd run instead of every run. This is an optimization, not a v1 requirement.

### Blocked Sources (Opt-Out Durability)

New graph label `BlockedSource`:

```rust
pub struct BlockedSource {
    pub url_pattern: String,    // exact URL or domain pattern
    pub blocked_at: DateTime<Utc>,
    pub reason: String,         // "opt-out", "spam", "privacy"
}
```

Checked before any Source node creation. Survives across discovery cycles.

### Org Mapping for Discovered Sources

**v1: Domain fallback** — the existing `resolve_org()` extracts the domain as a fallback org ID. This is imperfect (same org's website + Instagram treated as different orgs) but acceptable for the first iteration.

**v2 (future):** LLM-assisted org identity resolution — when the gap analyzer suggests a URL, it can also suggest `"parent_org": "tchabitat.org"` based on signal content. This would create dynamic OrgMapping entries.

### Pipeline Integration Point

In `scout.rs` `run_inner()`, after the clustering phase (line ~344 today):

```rust
// 5. Clustering
// ... existing clustering code ...

// 6. Source Discovery (NEW)
info!("Starting source discovery...");
let discoverer = SourceDiscoverer::new(
    self.graph_client.clone(),
    &self.anthropic_api_key,
    &self.profile.name,
);
match discoverer.run().await {
    Ok(discovery_stats) => {
        info!("{discovery_stats}");
    }
    Err(e) => {
        warn!(error = %e, "Source discovery failed (non-fatal)");
    }
}
```

Non-fatal — exactly like clustering. If discovery fails, the run still succeeded.

### Safety Considerations

- **Sensitivity-aware gap filling:** The gap analyzer prompt explicitly prohibits queries designed to identify individuals. But filling gaps in enforcement/sanctuary signal coverage IS allowed per principle: "Muting that signal out of paternalistic caution silences the people trying to help."
- **No private content:** Prompt instructs LLM not to suggest login-walled or private sources.
- **Geographic fuzziness:** Applied at display layer (existing `fuzz_node()` in reader.rs), not at source level. Discovered sources for sensitive topics get the same treatment as curated ones.
- **No user profiles:** Source discovery is system-internal. No user behavior influences what gets discovered.

### Astroturfing Defense — Investigation Reveals Absence

The system has three layers of structural defense:

**Layer 1: Org-diversity-driven velocity.** Story velocity is driven by `org_count` growth, not raw signal count. Flooding from one source doesn't move the needle.

**Layer 2: Evidence-based trust.** When the Investigator examines signals from a suspicious source, it looks for institutional depth:
- Is there a 501(c)(3) registration? → Evidence node
- Do media outlets mention this organization? → Evidence node
- Are there government grant records? → Evidence node
- Is there a physical address that checks out? → Evidence node

A real organization produces a rich web of Evidence. An astroturf operation produces *absence* — no registration, no media trail, no grant history, no institutional depth. The Investigator doesn't need a spam filter. It just asks questions, and the answers (or lack thereof) become the evidence record.

**Layer 3: Graph isolation.** Fake sources produce signals that don't connect to existing story clusters, don't get corroborated by independent orgs, and have no history in the graph. These signals naturally stay at low energy. You can't fake a web of independent corroboration backed by verifiable evidence — you'd need to compromise dozens of independent organizations *and* plant matching public records simultaneously.

## Implementation Phases

### Phase 1: Source Node Foundation

- [ ] Add `SourceNode`, `BlockedSource` structs to `types.rs`
- [ ] Add `Source` and `BlockedSource` graph labels to `migrate.rs`
- [ ] Add `create_source()`, `get_active_sources()`, `update_source_metrics()`, `deactivate_source()`, `is_blocked()` to `writer.rs`
- [ ] Seed existing `CityProfile` sources as Source nodes on first run (one-time migration)
- [ ] Modify `run_inner()` to load discovered sources alongside CityProfile
- [ ] Update `store_signals()` to increment per-source metrics
- [ ] **Gate:** Scout run works identically to today, but sources are now tracked as graph nodes. Stats show source-level breakdown.

### Phase 2: Investigation Framework

The system asks WHY about interesting signals, tensions, and newly discovered sources. This is Loop 2 from the architecture brainstorm — the Investigator.

- [ ] Create `modules/rootsignal-graph/src/investigate.rs`
- [ ] Define investigation triggers: new tension detected, new source discovered, signal with high urgency but low evidence
- [ ] Implement `Investigator` struct with LLM-driven evidence chain following:
  - Given a signal or source, generate targeted search queries (e.g., "Is [org name] a registered 501(c)(3)?", "[org name] media coverage")
  - Execute queries via Serper
  - Parse results into Evidence nodes with `source_url`, `content_hash`, `snippet`
  - Create `SOURCED_FROM` edges linking Evidence → Signal
- [ ] Add `ActorNode` type for organizations/entities discovered through investigation (future: dynamic OrgMapping)
- [ ] Wire into `run_inner()` after gap analysis — investigate newly discovered sources and high-interest signals
- [ ] Budget: max 10 investigation queries per run (web search cost control)
- [ ] **Gate:** After a scout run with investigation, Evidence nodes appear connected to signals from both curated and discovered sources. Sources with rich evidence trails are distinguishable from thin ones.

### Phase 3: Gap Analysis + Discovery + Evidence-Based Trust

- [ ] Create `modules/rootsignal-graph/src/discover.rs`
- [ ] Implement `SignalLandscape` assembly from graph queries
- [ ] Implement gap analysis LLM call (Haiku) with structured output
- [ ] Implement Serper query execution for gap-generated queries
- [ ] Implement URL validation (reachability check before source creation)
- [ ] Add blocked source check before creation
- [ ] Wire into `run_inner()` after clustering
- [ ] Add `DiscoveryStats` to `ScoutStats` output
- [ ] Implement `compute_trust()` based on evidence density (see Trust Lifecycle section)
- [ ] Implement deactivation logic for dead sources (consecutive_empty_runs >= 10)
- [ ] Add source trust trajectory to stats output
- [ ] Add `api_sources` endpoint to web server (for observability)
- [ ] **Gate:** After 5+ runs with investigation: sources with rich evidence have trust > 0.7. Sources with no evidence stay near their TLD baseline. Source list has grown organically. Evidence nodes form a verifiable trail for every trust score. A niche source with deep evidence outranks a popular source with none.

## Acceptance Criteria

### Functional Requirements

- [ ] Scout run loads discovered sources from graph alongside CityProfile seeds
- [ ] Gap analysis identifies at least 1 meaningful blind spot per run (when gaps exist)
- [ ] Discovered sources enter the scrape pipeline on the next run
- [ ] Per-source metrics (signals produced, corroborated) are tracked accurately
- [ ] Trust scores adjust based on evidence density (Evidence nodes connected to source's signals)
- [ ] Investigation produces Evidence nodes for newly discovered sources and high-interest signals
- [ ] Sources with `consecutive_empty_runs >= 10` are deactivated
- [ ] Blocked sources are never re-discovered
- [ ] Gap analysis respects budget limits (max 5 sources, max 3 queries per run)

### Non-Functional Requirements

- [ ] Gap analysis adds < 30 seconds to scout run (single Haiku call)
- [ ] Source loading adds < 1 second (simple graph query)
- [ ] Total discovered source count stays bounded (cap at 200 active per city)
- [ ] No PII in gap analysis queries or discovered source URLs
- [ ] Existing pipeline behavior unchanged when no discovered sources exist

### Quality Gates

- [ ] After 5 consecutive runs: at least 3 new sources discovered, at least 1 with Evidence nodes confirming institutional depth
- [ ] No regression in existing validation script (`validate-city-run.sh` still passes)
- [ ] Trust scores for curated sources with evidence remain stable or increase
- [ ] Source diversity increases: new domains appear in `source_domains` that weren't in CityProfile

## Success Metrics

1. **Source diversity growth:** Number of distinct source domains increases by 20%+ after 10 runs
2. **Audience coverage:** At least 1 previously-underrepresented audience role gets new signal from discovered sources
3. **Corroboration depth:** Average story `corroboration_depth` increases (more independent sources confirming signals)
4. **Signal surprise factor:** Discovered sources produce signals not present in the curated source set (new orgs, new programs)
5. **Trust convergence:** After 10+ runs, source trust scores have meaningful variance driven by evidence density, not formula tuning

## Dependencies & Risks

**Dependencies:**
- Serper API (already integrated, no new dependency)
- Anthropic API for gap analysis (already integrated via ai-client)
- Memgraph schema migration (idempotent, no downtime)

**Risks:**
- **LLM hallucinated URLs:** Mitigated by reachability check before source creation + natural trust decay if signals don't corroborate
- **Cost creep:** Mitigated by per-run budget caps (5 sources, 3 queries) and Haiku-class model
- **Org mapping gap:** Discovered sources won't have cross-platform org mappings, potentially inflating org_count. Mitigated by domain-fallback in resolve_org(). Acceptable for v1.
- **Source growth:** Mitigated by 200-source cap per city and deactivation logic
- **Scout lock timeout:** Gap analysis adds runtime. Mitigated by making discovery non-fatal and keeping it after clustering (lock can be released before discovery if needed)

## Alternative Approaches Considered

**1. Same-run discovery (discover and scrape in same run)**
Rejected: doubles pipeline complexity, extends scout lock duration, and makes the run non-idempotent (discovered sources change the run's own output). Next-run is simpler and more debuggable.

**2. Separate discovery service**
Rejected: adds operational complexity (another service to deploy/monitor). The scout already has all the infrastructure. Discovery is a natural extension of the pipeline, not a separate concern.

**3. Human-in-the-loop source approval**
Rejected for v1: adds friction that prevents emergent behavior. The evidence-based trust system IS the approval mechanism — automated, continuous, and grounded in verifiable facts. A human dashboard for monitoring is appropriate; human approval gates are not.

### Trust Models Considered and Rejected

Three trust models were designed, pressure-tested with adversarial scenarios, and rejected before arriving at evidence-based trust:

**4. Narrow corroboration rate (trust = signals_corroborated / signals_produced)**
Pressure-tested: robust against astroturfing (hard to fake cross-org corroboration) but creates a **corroboration paradox** — niche sources that serve underrepresented communities (Hmong mutual aid, Somali community orgs) produce valid signals that may never get corroborated by another source, simply because no other source covers that community. Pure corroboration rate penalizes exactly the sources the system should be discovering. Crude but robust → rejected because it penalizes the right sources.

**5. Graph embeddedness (5 metrics: evidence grounding, graph connectivity, story participation, cross-type references, source corroboration)**
Pressure-tested: elegant but gameable. A **piggyback attack** creates a source that produces a few real signals (copying from legitimate sources) to build graph connectivity, then starts injecting astroturf. A **collusion ring** of 3-4 sources that cross-corroborate each other can achieve high embeddedness scores. Also suffers from **embedding bias cascade** — sources producing signals in well-embedded topic areas score higher regardless of their own quality. Elegant → rejected because gameability undermines anti-fragility.

**6. Probationary trust (new sources start in probation, earn full trust after N corroborated signals)**
Pressure-tested: delays the corroboration paradox but doesn't solve it. Creates new attack surface: **probation cycling** (create source → produce noise → get deactivated → create new source → repeat, always in fresh probation). Also **story contamination during probation** — probationary signals still enter stories and can dilute quality. Crisis response too slow (legitimate crisis source stuck in probation during the crisis). Delays the problem → rejected because the problem needs dissolving, not delaying.

**Evidence-based trust (chosen):** Trust = evidence density. The Investigator asks WHY, follows evidence chains, produces Evidence nodes. Trust is literally the evidence that exists in the graph. No formula to game, no paradox to manage. A niche source with deep institutional evidence (501(c)(3) registration, media mentions, grant records, physical address) earns trust without needing corroboration from other sources. An astroturf source has no institutional depth to find. The absence of evidence IS the signal.

## Files to Modify

| File | Change |
|------|--------|
| `modules/rootsignal-common/src/types.rs` | Add `SourceNode`, `BlockedSource`, `SourceType`, `DiscoveryMethod` |
| `modules/rootsignal-graph/src/migrate.rs` | Add Source/BlockedSource constraints and indexes |
| `modules/rootsignal-graph/src/writer.rs` | Add source CRUD, metrics update, blocked check |
| `modules/rootsignal-graph/src/lib.rs` | Export new `discover` module |
| `modules/rootsignal-graph/src/discover.rs` | **NEW** — SourceDiscoverer, gap analysis, discovery pipeline |
| `modules/rootsignal-graph/src/investigate.rs` | **NEW** — Investigator, evidence chain following, Evidence node creation |
| `modules/rootsignal-scout/src/scout.rs` | Load discovered sources, wire discovery phase, update source metrics |
| `modules/rootsignal-scout/src/sources.rs` | Add `source_trust()` as public (already is), used by discover.rs |
| `modules/rootsignal-web/src/main.rs` | Add `api_sources` endpoint for observability |
| `modules/rootsignal-graph/Cargo.toml` | Add `schemars` if not already present (for GapAnalysisResponse) |

## References

### Internal References
- Architecture brainstorm: `docs/brainstorms/2026-02-16-civic-intelligence-system-architecture-brainstorm.md` — "Loop 1: Sense" describes the vision: "not scraping a fixed list of sources — *searching*... it hunts"
- Principles: `docs/vision/principles-and-values.md` — Principle #13: "Emergent Over Engineered"
- Milestones: `docs/vision/milestones-and-gates.md` — Milestone 5 describes manual source expansion; this feature makes it emergent
- Threat model: `docs/vision/adversarial-threat-model.md` — structural mitigations that discovered sources must respect
- Editorial principles: `docs/vision/editorial-and-signal-inclusion-principles.md` — inclusion test (civic, grounded, connected to action)
- Current sources: `modules/rootsignal-scout/src/sources.rs` — CityProfile struct, source_trust(), resolve_org()
- Scout pipeline: `modules/rootsignal-scout/src/scout.rs` — run_inner() phases, store_signals()
- Clustering: `modules/rootsignal-graph/src/cluster.rs` — Clusterer::run(), fetch_signal_metadata()
- Graph writer: `modules/rootsignal-graph/src/writer.rs` — create_node(), corroborate(), find_duplicate()

### Design Principles Applied
- **Emergent over engineered:** Sources emerge from signal patterns, not editorial judgment
- **No gatekeeping:** Low-trust sources still enter the graph; trust affects surfacing, not admission
- **Anti-fragile by structure:** Astroturfing detected by absence of evidence (no institutional depth), not by content filtering
- **Trust is evidence:** No formula to game. Trust is the literal Evidence nodes in the graph. Investigation produces evidence; evidence IS trust
- **Privacy by architecture:** No user behavior influences discovery; geographic fuzziness on sensitive signal
- **Organism metaphor:** The system grows its own senses based on what it already perceives
