# Scout vs Vision: Alignment Analysis

Pressure test of `rootsignal-scout` against `docs/vision/`. Conducted 2026-02-26 after the pipeline/ reorganization. Corrected same day after git history analysis revealed most "gaps" were implemented in other modules or exist but with call sites dropped during Story→Situation migration.

## What's Solidly Aligned

| Area | Grade | Notes |
|------|-------|-------|
| Signal extraction pipeline | A | 5 types (Gathering/Aid/Need/Notice/Tension) map cleanly to vision taxonomy. LLM-based, first-hand filter, quality scoring. |
| Multi-layer deduplication | A | Title match, in-memory embedding cache, graph vector index, cross-source corroboration. |
| Source discovery loop | A | Bootstrap, mid-run + end-of-run gap analysis, actor-linked account discovery, implied query expansion. Vision's "organism that grows its own senses" is implemented. |
| Event sourcing | A | Clean append-project pattern (Event → Postgres → Neo4j). Full audit trail + replay capability. |
| Testability | A | SignalStore + ContentFetcher traits. MOCK → FUNCTION → OUTPUT enforced. Deterministic embeddings. |
| Workflow orchestration | A | Restate durable execution, phase isolation, journaled side-effects. |
| Privacy by architecture | A | No user profiles, no query logging, no reporter identity. Sensitivity classification emitted as SystemDecision event. |
| Attribution | A | Every signal links to source_url. CitationNodes record provenance. |
| cause_heat propagation | A | `compute_cause_heat()` in rootsignal-graph, called by SupervisorWorkflow. Heat flows from Tensions through RESPONDS_TO edges. Evidence boost from EVIDENCE_OF edges. |
| Curiosity system | A | `trigger_situation_curiosity()` in graph writer, curiosity-driven discovery in `source_finder::discover_from_curiosity()`, investigation marking in `tension_linker`. |
| Budget tracking | B+ | Per-operation costs, daily limits, atomic spend tracking. |
| Actor graph | B+ | Extraction, linking, location triangulation. |
| Domain-based source trust | B | `is_source_trusted()` in severity_inference.rs with domain-type baseline scoring. Used in notice severity inference. |
| Scheduling | B | Weight-based cadence, exploration sampling (10% of slots). |

## Dropped During Story→Situation Migration

These features existed in the StoryWeaver / old pipeline and were lost when Story nodes became Situation nodes.

### 1. ~~merge_duplicate_tensions — Call Site Dropped~~ RESTORED

**Status: Fixed**

Call site restored in `run_supervisor_pipeline()` (step 2, before cause_heat). Cypher updated from stale `Story/CONTAINS` to `Tension-[:PART_OF]->Situation` to match the Situation architecture. Integration test `merge_duplicate_tensions_repoints_situation_edges` covers the new edge direction.

### 2. Situation Energy Scoring — Deleted, Not Migrated

**Priority: Medium**

**What existed:** `story_energy()` in the deleted `story_metrics.rs` combined:
- Velocity (40%) — rate of new signals joining
- Recency (20%) — freshness of latest signal
- Source diversity (10%) — distinct sources contributing
- Triangulation (20%) — type diversity (Tension + Aid + Gathering > 3x Tension)
- Channel diversity (10%) — web + social + RSS vs single channel

This was the primary ranking signal for stories in the reader.

**Current state:** Nothing equivalent exists for Situations. Reader sorts solely by `cause_heat DESC, last_confirmed_active DESC`. No velocity, no triangulation bonus.

**Impact:** A high-diversity Situation (Tensions + Aids + Needs + Gatherings all responding) no longer outranks a single-type cluster with higher raw heat. The system loses the ability to distinguish well-understood, multi-faceted community issues from single-source noise with high tension heat.

### 3. Situation Status Classification — Deleted, Not Migrated

**Priority: Medium**

**What existed:** `story_status()` in `story_metrics.rs` classified stories as:
- **Echo** — low type diversity, few entities (echo chamber, single-source repetition)
- **Confirmed** — high type diversity, multiple entities (triangulated, real)
- **Emerging** — moderate signals, growing

**Current state:** Echo detection exists only in `rootsignal-scout-supervisor/src/checks/echo.rs` as a supervisor check that flags issues but does **not** downrank signals or tag Situations.

**Impact:** No structural distinction between echo-chamber signal clusters and genuinely triangulated community issues. The reader serves both equally.

### 4. Triangulation-Based Sort in Reader — Replaced with Heat-Only

**Priority: Low (follows from #2)**

**What existed:** `list_recent` and `find_nodes_near` sorted by `story_triangulation DESC` first, then `cause_heat`. Type diversity was the primary sort key.

**Current state:** Sort is `cause_heat DESC, confidence DESC` or `cause_heat DESC, last_confirmed_active DESC`.

**Impact:** Follows automatically from restoring energy scoring (#2). Not a separate fix.

## Remaining True Gaps

### 5. Evidence-Based Source Trust — Never Fully Implemented

**Priority: Low (for current stage)**

**Vision says:** Sources accumulate evidence (501(c)(3) registrations, media mentions, grants, physical presence). Trust converges toward evidence density.

**What exists:** Domain-based baseline trust in `is_source_trusted()` (.gov, .org, etc.) and EVIDENCE_OF edges used in cause_heat boosting. But no Investigator that actively follows evidence chains for sources, no trust score that converges from evidence accumulation.

**Assessment:** The domain-based trust + EVIDENCE_OF edge infrastructure is a reasonable foundation. The active investigation system described in the vision (Investigator follows evidence chains) was never built — it's a planned feature, not a regression.

### 6. Engagement-Aware Discovery — Implicit Only

**Priority: Low**

**Vision says:** Engagement score ranks tensions for discovery budget. Reserve 2+ queries for low-engagement tensions.

**What exists:** SourceFinder's DiscoveryBriefing includes gap stats and the LLM implicitly prioritizes. No explicit scoring formula or mechanical budget reservation.

### 7. Crisis Mode — Not Implemented

**Priority: Low (foundation not affected)**

Single cadence-based scheduling. No crisis detection or mode switching.

## Not Gaps (Scout's Role is Correct)

| Feature | Where It Lives | Scout's Role |
|---------|---------------|--------------|
| Resource matching queries | API/query layer | Scout writes REQUIRES/PREFERS/OFFERS edges correctly |
| Sensitivity-based geo fuzziness | API serving layer | Scout classifies and stores SensitivityLevel correctly |
| Situation clustering | rootsignal-graph (Leiden) | Scout consumes cluster results via DiscoveryBriefing |
| Power Scout (structural analysis) | Separate future crate | Vision explicitly says different evidentiary bar, pacing, sources |

## Verdict

Scout is well-aligned with the vision. The initial analysis overcounted gaps — cause_heat, curiosity filtering, and domain-based trust all exist, just in rootsignal-graph rather than rootsignal-scout.

The real issue was **migration regressions from Story→Situation**:

1. ~~**merge_duplicate_tensions**~~ — **RESTORED.** Call site re-added, Cypher fixed for Situation/PART_OF.
2. **Situation energy scoring** — needs a new `situation_energy()` or equivalent to restore multi-factor ranking (velocity, triangulation, diversity)
3. **Situation status classification** — needs echo/confirmed/emerging tagging restored for Situations

Items 2-4 were **replaced** by the Situation architecture (`situation_temperature.rs`, `SituationArc`, `Clarity`, `echo_score`), though the ranking formula may still benefit from the original multi-factor weighting. Everything else is either implemented, correctly deferred, or a future-stage optimization.
