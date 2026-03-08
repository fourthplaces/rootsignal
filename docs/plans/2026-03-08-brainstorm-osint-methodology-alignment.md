# Brainstorm: OSINT Methodology Alignment

## The Question

How much of RootSignal's design can borrow from decades of OSINT (Open Source Intelligence) methodology? Where does the vision overlap, and where does it diverge?

---

## Where RootSignal Already Maps to OSINT

| OSINT Phase | RootSignal Equivalent | Notes |
|---|---|---|
| **Collection Planning** | Topic discovery, source management | OSINT formalizes this as "collection requirements" — standing priorities that drive what gets collected |
| **Collection** | Web scrape, social scrape | Direct match. OSINT distinguishes passive (monitoring) from active (targeted) collection |
| **Processing/Exploitation** | Signal extraction, dedup, actor extraction | Turning raw material into structured intelligence products |
| **Analysis** | Curiosity domain, situation weaving | Pattern-finding across sources — this is where OSINT has the deepest methodology |
| **Dissemination** | Graph projection, API, admin UI | Making intelligence consumable for decision-makers |
| **Feedback** | (Gap) | OSINT closes the loop: consumers tell collectors what was useful, refining future collection |

---

## High-Value Borrowings

### 1. Two-Axis Source Evaluation (Admiralty System)

OSINT separates **source reliability** from **information credibility**:

- **Source reliability** (A–F): Track record of the source over time. Has this Instagram account, news outlet, or community board been accurate before?
- **Information credibility** (1–6): How believable is this specific claim? Corroborated by other sources? Internally consistent?

RootSignal currently has `confidence` as a single score. Splitting this into two axes would let us say: "This source is generally reliable (B), but this specific claim is unconfirmed (4)." That's a much richer signal for downstream consumers.

### 2. Indicators & Warnings (I&W)

OSINT defines **indicator sets** — specific observable signals that, when detected, suggest something is happening. Example: a gentrification indicator set might include "new coffee shop openings," "rent increase complaints," "long-time business closures," "demographic shift mentions."

The curiosity domain could adopt this: define indicator sets per topic/concern, then actively watch for them rather than discovering everything from scratch each run. This turns curiosity from purely exploratory into a mix of directed monitoring + open discovery.

### 3. Standardized Confidence Language

OSINT has converged on probability language:

| Term | Probability Range |
|---|---|
| Almost certain | 90–100% |
| Highly likely | 80–90% |
| Likely | 55–80% |
| Realistic possibility | 25–55% |
| Unlikely | 15–25% |
| Highly unlikely | 0–15% |

Mapping `confidence` scores to these terms gives downstream consumers (and LLMs doing further analysis) a shared vocabulary.

### 4. Structured Analytic Techniques for Situation Weaving

OSINT's **Analysis of Competing Hypotheses (ACH)** is directly relevant to situation weaving:

- When multiple narratives explain the same signals, formally list hypotheses
- Score each signal as consistent/inconsistent/neutral for each hypothesis
- The hypothesis with the fewest inconsistencies wins (not the most confirmations — this fights confirmation bias)

This could make situation weaving more rigorous and auditable.

### 5. Collection Management / Requirements

OSINT distinguishes:
- **Standing requirements**: Ongoing priorities ("always monitor for housing displacement signals in this area")
- **Ad-hoc requirements**: One-time requests ("investigate this specific event")
- **Essential Elements of Information (EEI)**: The specific questions collection should answer

RootSignal's scout runs are currently more "collect everything and see what's there." Adding formal collection requirements would let the system focus effort where it matters most.

---

## Architectural Alignment Audit

A deep look at the event sourcing architecture reveals how closely the existing design maps to OSINT patterns — and where the gaps are structural, not just conceptual.

### What's Already Strong

**Three-layer event taxonomy = OSINT intelligence product tiers.**
The World/System/Telemetry split maps directly to how OSINT separates raw intelligence (RAWS), finished intelligence (FINTEL), and collection logs. Most OSINT tools don't achieve this separation cleanly — RootSignal does it at the architectural level.

| OSINT Product Tier | RootSignal Layer | Example |
|---|---|---|
| Raw Intelligence (RAWS) | WorldEvents | `GatheringAnnounced`, `ConcernRaised` — immutable observed facts |
| Finished Intelligence (FINTEL) | SystemEvents | `CategoryClassified`, `SeverityClassified` — editorial judgments |
| Collection Logs | TelemetryEvents | `UrlScraped`, `LlmExtractionCompleted` — infrastructure noise |

**Multi-source validation is built into the pipeline.** The dedup layer's three outcomes (Created, Corroborated, Refreshed) are textbook intelligence correlation. `CitationPublished` creates an explicit evidence chain. `corroboration_count`, `source_diversity`, and `channel_diversity` on `NodeMeta` give every signal a multi-source confidence profile. OSINT calls this "all-source analysis" — RootSignal does it automatically during dedup.

**Intelligence lineage via causal chains.** `parent_seq`/`caused_by_seq` on every event means any conclusion can be traced back to its source evidence. This is stronger than most OSINT tools, which often lose provenance during analysis.

**Corrections preserve the record.** `GatheringCorrected`, `ConcernCorrected`, etc. as events (not mutations) means the original intelligence is never lost. OSINT calls this "intelligence revision" — you can always see what was originally reported and how it was later refined.

**Source management has the bones of collection management.** `SourceNode` tracks `signals_produced`, `signals_corroborated`, `quality_penalty`, `weight`, `avg_signals_per_scrape`, `consecutive_empty_runs`. This is source performance tracking — a prerequisite for formal collection management.

### Structural Gaps

**1. Confidence conflates source reliability and information credibility.**

`NodeMeta.confidence` (0.0–1.0) is a single axis. The investigator adjusts it based on evidence (+0.05 direct, -0.10 contradicting) — that's *information credibility*. But all signals start at `confidence: 0.5` regardless of source track record.

Meanwhile, `SourceNode.weight` and `quality_penalty` track source performance but feed into *collection scheduling*, not signal credibility. The data exists on both sides but isn't connected:

```
SourceNode.signals_corroborated / SourceNode.signals_produced  →  source reliability ratio
                                                                    (currently unused for signal scoring)

NodeMeta.confidence  →  information credibility
                        (currently doesn't factor in source track record)
```

A concrete fix: when dedup creates a signal, seed `confidence` from the source's historical corroboration rate instead of hardcoding 0.5. A source that's been corroborated 80% of the time should start its signals higher.

**2. No collection requirements feed into the scout pipeline.**

The scout run starts with `ScoutRunRequested` → `SourcesPrepared`, which builds a source plan from *all active sources in the region*. There's no mechanism for saying "prioritize housing signals this run" or "we specifically need to know about shelter capacity."

`DemandReceived` and `PinCreated` exist as SystemEvents but don't feed back into `SourcesPrepared` logic. The feedback loop is structurally absent — consumers can express interest but it doesn't change what gets collected or how.

OSINT would model this as:
- `CollectionRequirement` entity (standing or ad-hoc) with priority + EEI
- `SourcesPrepared` handler consults active requirements when building the source plan
- Post-run evaluation: which requirements were satisfied? Which have gaps?

**3. Situation weaving is topic clustering, not analytic assessment.**

`pure.rs` clusters by cosine similarity (threshold 0.6). This groups *related* signals but doesn't assess *competing explanations*. Two situations about the same topic with contradictory narratives would just merge if their embeddings are similar enough.

OSINT's ACH asks: "What are the possible explanations? Which evidence is *diagnostic* (distinguishes between hypotheses) vs. *redundant* (consistent with all of them)?" The current approach can't distinguish between a situation where all evidence points one way vs. where evidence is genuinely contested.

**4. Curiosity is purely exploratory — no directed monitoring.**

`concern_linker.rs` does open-ended agentic investigation: generate queries, search, evaluate. This is OSINT's "collection" phase. But there's no "indicators and warnings" phase where the system watches for *predefined patterns* that would trigger investigation.

The signal taxonomy (Gathering, Resource, HelpRequest, Concern, Condition, Announcement) already provides a classification framework. Indicator sets would add a layer on top: "when we see 3+ Concerns about rent in the same ZIP within 2 weeks, that's a displacement indicator — investigate immediately."

**5. Channel types exist but don't weight credibility.**

`ChannelType` (Press, Social, DirectAction, CommunityMedia) is tracked per citation and used for `channel_diversity` scoring. But a press citation and a social media post are weighted equally for corroboration. OSINT would weight them differently — a press report corroborating a social media rumor is stronger evidence than two social media posts saying the same thing.

---

## Where RootSignal Diverges from Traditional OSINT

### Community-centric, not threat-centric
Traditional OSINT serves security/military/law-enforcement — it's threat-focused. RootSignal is community-focused: surfacing needs, resources, tensions, and conditions to help communities self-organize. The methodology transfers; the *framing* is fundamentally different.

### Public benefit, not intelligence advantage
OSINT traditionally creates information asymmetry (the analyst knows more than others). RootSignal's goal is the opposite — making community intelligence *legible and accessible* to everyone in the community. This changes dissemination patterns entirely.

### Signal taxonomy is domain-specific
OSINT categorizes by source type (HUMINT, SIGINT, IMINT, etc.). RootSignal categorizes by *what the information means to a community* — Concerns, Conditions, Resources, HelpRequests, Announcements. This taxonomy is a strength; it shouldn't be replaced by OSINT categories.

### Ethical constraints are tighter
OSINT practitioners often operate in gray areas (scraping private social media, correlating identities). RootSignal should hold a higher bar — only truly public information, with sensitivity controls and awareness that community members are subjects, not targets.

---

## Open Questions

1. **Should we adopt OSINT terminology internally?** E.g., calling scrape sources "collection assets," calling situation weaving "all-source analysis." Pro: leverages existing body of knowledge. Con: might import militaristic framing that doesn't fit the mission. The community-centric framing is a feature, not a bug.

2. **How far should indicator sets go?** Full I&W feels powerful but complex. A lighter version: each Concern type has a set of "watch for" keywords/patterns that bias collection priority. Could start as a simple config (TOML/YAML indicator definitions) rather than a full event-driven subsystem.

3. **Is ACH overkill for situation weaving?** It adds rigor but also complexity. A simplified version: when weaving, the LLM must explicitly state what evidence *contradicts* its narrative, not just what supports it. This is one prompt change, not a new system.

4. **Feedback loop priority?** `DemandReceived` and `PinCreated` already exist as SystemEvents. The gap is wiring them into `SourcesPrepared`. Should this be a near-term priority, or does it depend on having real consumers first?

5. **Source-seeded confidence: how aggressive?** The data for source reliability already exists (`signals_corroborated / signals_produced`). The question is how much it should influence initial confidence. A source with 90% corroboration rate seeding signals at 0.7 instead of 0.5 could be powerful — or could entrench bias toward established sources.

6. **Channel-weighted corroboration: worth the complexity?** A press citation corroborating a social post is stronger than two social posts. But weighting channels means making editorial judgments about which channels matter more — which cuts against the community-centric mission. Is channel diversity (already tracked) sufficient, or do we need channel *authority*?

7. **What would "intelligence products" look like?** OSINT produces specific deliverables (situation reports, threat assessments, briefings). RootSignal's situations are the closest analog. Should we formalize situation reports as a first-class output format?

---

## Possible Next Steps

### No-code changes (framing only)
- Map confidence scores to standardized probability language terms. Add a `confidence_label()` helper that converts 0.72 → "Likely". Improves interpretability for consumers and LLMs immediately.
- Document the OSINT alignment in architecture docs so future contributors understand the lineage.

### Small code changes
- **Source-seeded confidence**: When dedup creates a signal, compute `source_corroboration_rate = signals_corroborated / signals_produced` and seed `confidence` from it instead of hardcoding 0.5. One change in the dedup handler.
- **Contradicting evidence in weaving**: Add a prompt requirement to situation weaving — "list evidence that contradicts this narrative." ACH-lite with zero infrastructure.

### Medium effort
- **Channel-weighted corroboration**: When `CorroborationScored` fires, weight by channel type. A Press citation counts more than a second Social citation for the same signal.
- **Demand-driven collection**: Wire `DemandReceived`/`PinCreated` into `SourcesPrepared` so consumer interest influences source priority and collection focus.

### Larger effort
- **Two-axis evaluation**: Split `confidence` into `source_reliability` (on SourceNode, historical) and `information_credibility` (on NodeMeta, per-signal). Requires schema changes, projector updates, and UI changes.
- **Indicator sets**: Define indicator patterns (co-occurring signal types + categories + geography + time window) that trigger automatic investigation. New subsystem, but could start as config-driven rules.

### Largest effort
- **Collection requirements as first-class entities**: `CollectionRequirement` events with priority, EEI, and satisfaction tracking. Fundamentally changes scout run planning from "scrape everything" to "satisfy requirements."
- **Formal ACH in situation weaving**: Replace embedding similarity with hypothesis-driven clustering. Each situation carries competing hypotheses scored against diagnostic evidence. Major rearchitecture of `pure.rs`.
