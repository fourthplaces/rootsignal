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

## Brainstorms

Architectural exploration. The canonical brainstorm supersedes earlier versions.

| Document | Status | Description |
|----------|--------|-------------|
| [Civic Intelligence System Architecture](brainstorms/2026-02-16-civic-intelligence-system-architecture-brainstorm.md) | canonical | **The architecture document.** Agent swarm, Neo4j graph, phased rollout |
| [Civic Tension Search Engine Architecture](brainstorms/2026-02-16-civic-tension-search-engine-architecture-brainstorm.md) | superseded | Earlier architecture focused on tension-only. Superseded by the civic intelligence system brainstorm |
| [Civic Tension Search Engine](brainstorms/2026-02-16-civic-tension-search-engine-brainstorm.md) | superseded | Initial concept brainstorm. Superseded by the architecture brainstorms |

---

## Naming Convention

The product is **Root Signal**. The repository is `taproot`. All documents should use "Root Signal" when referring to the product/system.

## Scope Discipline

The reference docs and landscape docs describe the *aspirational* breadth of Root Signal — all possible sources, all possible products. The milestones and kill-test define the *actual* discipline: what gets built when, and what earns the right to the next phase. When in doubt, follow the milestones.

**Now:** Phase 1a — scout agent → graph → web surface → quality measurement. Proving signal exists.
**Next:** Phase 1b — investigator agent, response discovery, Restate orchestration, filtering UI.
**Later:** Phases 2-4 — community needs, ecological stewardship, ethical consumption, national scale.

The source list and ecosystem doc are *menus*, not *commitments*. Nothing from those lists enters the system until the current phase is solid.
