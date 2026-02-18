# Root Signal — Documentation Index

## Document Status Labels

- **canonical** — The authoritative version. Follow this.
- **draft** — Work in progress. May change significantly.
- **reference** — Stable reference material. Not prescriptive.
- **superseded** — Replaced by a newer document. Kept for historical context.

---

## Vision

The soul of the project. These define *what* Root Signal is, *why* it exists, and the principles that guide every decision.

| Document | Status | Description |
|----------|--------|-------------|
| [Principles & Values](vision/principles-and-values.md) | canonical | Core beliefs, principles, anti-principles, and decision framework |
| [Editorial & Signal Inclusion](vision/editorial-and-signal-inclusion-principles.md) | canonical | What Root Signal includes, excludes, and why. Normal vs crisis mode |
| [Alignment Machine](vision/alignment-machine.md) | canonical | The emergent property: how the graph reflects civic alignment |
| [Problem Space & Positioning](vision/problem-space-positioning.md) | canonical | Where Root Signal sits in the world |
| [Milestones & Gates](vision/milestones-and-gates.md) | canonical | Sequential milestones with go/no-go gates |
| [Kill Test](vision/kill-test.md) | canonical | Every failure mode, honestly assessed |
| [Adversarial Threat Model](vision/adversarial-threat-model.md) | canonical | How the system prevents being used as a weapon against the people it serves |

## Landscape

How Root Signal relates to what already exists and what could be built on top.

| Document | Status | Description |
|----------|--------|-------------|
| [Adjacent & Overlapping Systems](landscape/adjacent-overlapping-systems.md) | canonical | Existing platforms, what they do, where they fall short |
| [Ecosystem: What Gets Built On Top](landscape/ecosystem-what-gets-built-on-top.md) | canonical | Applications and integrations that emerge from the signal substrate |

## Reference

Implementation-agnostic reference material that carries forward regardless of architecture.

| Document | Status | Description |
|----------|--------|-------------|
| [Signal Sources & Roles](reference/signal-sources-and-roles.md) | reference | 70+ signal sources, audience roles, quality dimensions, scraping cadence |
| [Pressure Test Queries](reference/pressure-test-queries.md) | reference | 150+ real-world queries organized by domain, cross-domain, edge cases, and heat map |
| [Civic Engagement Landscape](reference/civic-engagement-landscape.md) | reference | 12 engagement domains + engagement types. Scout guidance for what to look for |
| [Audience × Causes Map](reference/audience-causes-map.md) | reference | 9 Twin Cities audience archetypes with causes and cross-domain surprise connections |
| [Use Cases](reference/use-cases.md) | reference | Concrete use cases and user stories for Root Signal |

## Plans

Implementation plans for features. Plans describe intended phases; check the codebase for actual build status.

| Document | Status | Description |
|----------|--------|-------------|
| [Phase 1a Implementation](plans/phase-1a-implementation.md) | superseded | Original Phase 1a plan. Core loop is built; investigator, editions, supervisor, and API now exist beyond this plan's scope |
| [Cause Heat](plans/2026-02-17-feat-cause-heat-plan.md) | canonical | Cross-story signal boosting. **Fully implemented** |
| [Community Signal Scoring](plans/2026-02-17-feat-community-signal-scoring-plan.md) | canonical | Source diversity and external ratio. **Fully implemented** |
| [Scout Supervisor](plans/2026-02-17-feat-scout-supervisor-plan.md) | canonical | Supervisor auto-fixes and health checks. **Phase 1 implemented**, Phases 2-5 pending |
| [Emergent Source Discovery](plans/2026-02-17-feat-emergent-source-discovery-plan.md) | canonical | Self-expanding signal via gap analysis. **Phase 1 (source nodes) implemented**, Phases 2-3 pending |
| [Instagram Hashtag Discovery](plans/2026-02-17-feat-individual-signal-discovery-via-instagram-hashtags-plan.md) | canonical | Individual signal discovery via hashtags. **Partial** |

## Brainstorms

Architectural exploration. The canonical brainstorm supersedes earlier versions.

| Document | Status | Description |
|----------|--------|-------------|
| [Civic Intelligence System Architecture](brainstorms/2026-02-16-civic-intelligence-system-architecture-brainstorm.md) | canonical | **The architecture document.** Agent swarm, Memgraph graph, phased rollout |
| [Civic Tension Search Engine Architecture](brainstorms/2026-02-16-civic-tension-search-engine-architecture-brainstorm.md) | superseded | Earlier architecture focused on tension-only |
| [Civic Tension Search Engine](brainstorms/2026-02-16-civic-tension-search-engine-brainstorm.md) | superseded | Initial concept brainstorm |
| [Anti-Fragile Signal](brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md) | draft | Letting truth emerge under pressure |
| [Cause Heat](brainstorms/2026-02-17-cause-heat-brainstorm.md) | draft | Cross-story signal boosting exploration |
| [Community Signal Scoring](brainstorms/2026-02-17-community-signal-scoring-brainstorm.md) | draft | Source diversity and trust scoring |
| [Email Ingest](brainstorms/2026-02-17-email-ingest-for-signal-brainstorm.md) | draft | Email as a signal capture channel |
| [Individual Signal Discovery](brainstorms/2026-02-17-individual-signal-discovery-brainstorm.md) | draft | Instagram-based individual signal discovery |
| [Radio as Signal Source](brainstorms/2026-02-17-radio-as-civic-signal-source-brainstorm.md) | draft | Radio as a civic signal source |
| [Scout Supervisor](brainstorms/2026-02-17-scout-supervisor-brainstorm.md) | draft | Autonomous scout health management |
| [Signal vs. Affordances](brainstorms/2026-02-17-signal-vs-affordances-brainstorm.md) | draft | What Root Signal is and isn't |
| [Triangulation Model](brainstorms/2026-02-17-triangulation-model-brainstorm.md) | draft | From echo to structural truth |

## Tests

Testing playbooks for manual and integration testing.

| Document | Description |
|----------|-------------|
| [Scout Testing](tests/scout-testing.md) | Scout pipeline end-to-end testing |
| [Investigation Testing](tests/investigation-testing.md) | Investigation framework testing (cooldown, dedup, evidence) |
| [Source Registry Testing](tests/source-registry-testing.md) | Source node foundation testing |

## Audits

| Document | Description |
|----------|-------------|
| [Evidence Surfacing Audit](audits/2026-02-17-evidence-surfacing-audit.md) | Web layer evidence surfacing review |

## Solutions

Documented learnings from solved problems.

| Document | Description |
|----------|-------------|
| [unwrap_or Masks Data Quality](solutions/2026-02-17-unwrap-or-masks-data-quality.md) | Anti-pattern: using defaults that hide missing data |

---

## Naming Convention

The product is **Root Signal**. The repository is `rootsignal`. All documents should use "Root Signal" when referring to the product/system.

## Scope Discipline

The reference docs and landscape docs describe the *aspirational* breadth of Root Signal — all possible sources, all possible products. The milestones and kill-test define the *actual* discipline: what gets built when, and what earns the right to the next phase. When in doubt, follow the milestones.

**Done:** Phase 1a — scout agent → graph → web surface → quality measurement. Core loop works.
**Now:** GraphQL API, scout supervisor (Phase 1), investigation framework, cause heat, community signal scoring.
**Next:** Emergent source discovery, supervisor notifications and feedback loops, filtering UI.
**Later:** Phases 2-4 — community needs, ecological stewardship, ethical consumption, national scale.

The source list and ecosystem doc are *menus*, not *commitments*. Nothing from those lists enters the system until the current phase is solid.
