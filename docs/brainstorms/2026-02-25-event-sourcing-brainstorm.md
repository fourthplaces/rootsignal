---
date: 2026-02-25
topic: event-sourcing
---

# Event Sourcing for RootSignal

## What We're Building

The event log is the foundation of the entire system — the most grounding piece of infrastructure. Everything else is downstream: the graph, the API, the admin, every future feature. Pre-launch is the time to get this right.

Refactor the existing scout run log into an append-only event stream that serves as the single source of truth. The Neo4j graph becomes a deterministic materialized view, rebuildable from the event stream. Every graph mutation must flow through an event — no side doors.

## Why This Approach

The system already has most of the pieces: `scout_run_events` captures typed, causally-nested events (SignalCreated, SignalCorroborated, LintCorrection, etc.), and the Neo4j graph is already a derived projection of pipeline decisions. The gap is that events are currently observability artifacts scoped to individual runs, not the authoritative source of truth.

Formalizing this gives us:

- **Full auditability** — every signal traces back through events to source URLs. The complete paper trail of how data was generated and where it came from.
- **Replayability** — blow away the graph, replay events, rebuild it. Replayability *proves* the audit trail is complete. If replay produces a different graph, we found a gap.
- **Antifragility** — the audit trail and replayability reinforce each other. Replayability ensures the audit trail is correct. The audit trail ensures we're capturing accurate information and nothing falls through the cracks.

## Key Design Decisions

### Events are facts, not commands

Events describe what happened in the world — not what to do to a database. They are completely decoupled from Neo4j, Postgres, or any infrastructure. They stand on their own. A human reading the event log should understand what the system observed and decided, without knowing anything about graph databases.

The LLM extraction and embedding-based dedup are stochastic — they happen *before* the append. Once a fact is in the log, it's immutable. The fuzzy world (LLMs, scrapers, embeddings) produces facts; the event log records them; consumers (graph reducer, debugging tools, AI analysis) interpret them.

```
Fuzzy world (LLMs, scrapers, embeddings)
         ↓ facts
   Event Log (append-only, universal fact stream)
         ↓                    ↓                ↓
   Graph Reducer         Debugging Tools    AI Analysis
   (Neo4j view)          (run inspection)   (pattern detection)
```

### One stream, not two

Observability events (url scraped, LLM extraction completed) and decision events (signal discovered, signal corroborated) are all facts. One unified stream. The graph reducer acts on the facts it cares about and ignores the rest. A debugging tool cares about all of them. An AI analysis tool might find patterns across the full history. The event log is a universal fact stream with multiple consumers.

### Consensus is verification against reality

Since all signals trace back to real-world sources (URLs, social posts, public records), "consensus" degrades to: can we still reach the source, and does it still say what we claimed? The existing `content_hash` on Citations and `SOURCED_FROM` edges already provide the primitives for this. Truth is the consensus algorithm.

### Global ordering independent of runs

Events need a monotonic sequence number independent of which scout run produced them, so the full stream can be replayed in order across all runs.

### Complete event coverage

Every graph mutation must produce an event — not just scout pipeline operations. This includes: reap_expired, admin review status changes, manual edits, and any future mutation path.

### Permissionless append with verified claims

Anyone can write to the event log — the scout pipeline, third-party integrations, external submissions — so long as the claim can be verified against its source. The gate isn't *who* submits, it's *can we check the source URL and confirm it says what was claimed?* This is the natural conclusion of "truth is the consensus algorithm."

This shifts the system from **gatekeeping** (should this event enter the log?) to **scoring** (how grounded is this claim?). Everything gets in. The graph reflects confidence levels. High-confidence signals surface. Low-confidence signals sink. Spam doesn't corrupt — it just doesn't rise.

### Immune system, not firewall

Bad inputs are invited in. The system gets stronger from exposure. Existing quality signals already measure groundedness in reality:

- **corroboration_count** — a signal confirmed by one source stays low. A real gathering confirmed by a newspaper, a Facebook event, and a Nextdoor post rises.
- **source_diversity + channel_diversity** — a bot farm across 10 Reddit accounts still looks like one channel type. Real signals show up across Press, Social, DirectAction, CommunityMedia.
- **content_hash verification** — bots pointing to bots creates a fragile citation graph. Sources change or disappear. The refresh cycle naturally erodes these.
- **source reputation** — not just "how many sources" but "how credible." A Press citation from a known news outlet carries more weight than a week-old Twitter account. This layers from coarse (ChannelType) to fine (entity-level reputation, actor scoring) over time.

Coordinated adversarial input that mimics diversity (fake news site + fake Facebook event + fake GoFundMe) is the hardest case — but that's the misinformation problem at large, not unique to this system. Source verification over time makes the attack expensive to sustain.

The holes in the system are features, not bugs. They were already there if we tried to gatekeep — we just couldn't see them. Every hole found makes the graph stronger.

### Facts only — no computed artifacts in events

Events carry what was observed, not what was computed. Embeddings, vector indexes, and derived metrics are reducer concerns. A signal's title and summary are facts. The embedding of that text is a computed representation for a specific infrastructure (Neo4j vector search). Keeping computed artifacts out of events means:

- Events are smaller and infrastructure-agnostic
- Upgrading the embedding model means re-reducing — the whole graph improves retroactively
- The fact layer has no dependency on embedding services or models

### The reducer is versioned, the events are forever

The event log is a permanent record. The reducer — the logic that projects events into the graph — evolves over time. When we add a new scoring dimension (actor reputation, financial transparency, whatever comes next), we re-reduce the entire log and the whole graph rebalances retroactively.

The events don't change. Our understanding of them deepens.

This means the system auto-balances over time. Add actor scoring? Re-reduce — every signal's confidence rebalances based on who submitted it. Add financial source tracking? Re-reduce — signals backed by transparent funding rise, opaque ones sink. Detect an imbalance? Investigate, add a new scoring dimension, re-reduce, the whole history adjusts.

The event log is the hypothesis space. The reducer is the current best model of reality. The graph is the output.

## What Changes From Today

1. **Global event sequence** — events get a monotonic ID independent of scout runs
2. **Complete coverage** — reap_expired, admin review, manual edits all become events
3. **Graph reducer** — a function that takes the event stream and produces the graph, replacing direct writes in `writer.rs`

## What Stays The Same

- The event types already captured (SignalCreated, Corroborated, LintCorrection, etc.)
- The scout pipeline and LLM-driven extraction
- Neo4j as the graph store
- Postgres as the event store (for now)

## What We're Deferring

- **Multi-server writes** — solvable with Postgres sequences when needed
- **Inter-service event schema** — not needed until services exist
- **Entity-level reputation scoring** — start with coarse ChannelType weighting, refine over time
- **Financial transparency tracking** — future scoring dimension, re-reduce when ready

## Antifragility Loop

Periodically: replay events → rebuild graph → diff against live graph. Any discrepancy reveals either a bug in the reducer or a missing event type. This is the test suite for system completeness.

## Open Questions

- Event retention policy — do events live forever, or do we compact/snapshot?
- Should the reducer be a separate binary/process, or inline in the existing pipeline?
- What does the replay diff test look like concretely? Full graph comparison, or spot checks?

## Next Steps

→ `/workflows:plan` for implementation details
