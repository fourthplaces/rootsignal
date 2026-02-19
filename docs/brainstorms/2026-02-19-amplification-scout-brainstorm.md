---
date: 2026-02-19
topic: amplification-scout
---

# Amplification Scout

## What We're Building

An amplifier pipeline stage that enhances the existing scout investigation
infrastructure. When Response Scout or Gravity Scout finds evidence that people are
actively engaging with a tension, the Amplifier re-runs the investigation loop with
enriched prompt context — seeded with what's already been discovered — to find ALL
other forms of engagement with that tension.

The key insight: a discovered response proves the tension is actionable. The
amplifier uses that proof to cast a wider, type-agnostic net.

## Why This Approach

We considered building a standalone scout with its own extraction and graph logic,
but the amplifier's value is purely in prompt construction. The search tools,
extraction, dedup, embedding, and edge creation are identical to what Response Scout
and Gravity Scout already use. A pipeline stage avoids duplicating that
infrastructure.

## How It Works

1. Response Scout finds: "Know Your Rights workshop responds to ICE enforcement fear"
2. Gravity Scout finds: "vigil at City Hall drawn to ICE enforcement fear"
3. Amplifier collects all known responses/gatherings for that tension
4. Re-invokes the investigation loop with enriched prompt context:
   - Known responses as grounding ("people are already doing X, Y, Z")
   - Explicit instruction to search broadly across ALL response types
   - Steers away from narrowing on the type already found
5. Findings flow through normal pipeline: dedup, embedding, edge creation

### Example Prompt Enrichment

**Without amplification (Response Scout today):**
> "What diffuses the tension: ICE enforcement fear?"

**With amplification:**
> "People are already responding to ICE enforcement fear — a Know Your Rights
> workshop was found, a vigil is happening at City Hall, a donation drive is
> collecting for affected families. Search broadly for ALL other ways people are
> engaging with this tension: volunteering, donating, organizing, legal aid,
> mutual aid, protests, community meetings, advocacy."

### Trigger

Runs after Response Scout and Gravity Scout complete. Any tension that received
at least one new RESPONDS_TO or DRAWN_TO edge during the current run is eligible
for amplification.

### No Recursion

The amplifier does not recurse on its own output. Instead, the enriched graph
feeds forward into the next run's normal scout passes. Response Scout and Gravity
Scout naturally benefit from the richer context on subsequent runs — the recursion
is the existing run loop.

## Key Decisions

- **Pipeline stage, not a separate scout**: Same tools, extraction, dedup, and
  graph edges. The only new thing is prompt construction.
- **Type-agnostic search**: The amplifier deliberately does NOT search for "more of
  the same" — it searches for all response types, using known responses as context
  rather than filters.
- **Feeds from both scouts**: Gravity and Response Scout findings both contribute
  prompt context. The amplifier doesn't care which scout found what.
- **No self-recursion**: Enrichment feeds forward into the next run's scout passes,
  not back into the amplifier.

## Open Questions

- **Budget**: Each amplification is ~3 Haiku calls + 5 searches + 3 page reads per
  tension. How many tensions get amplified per run? May need a threshold (e.g.,
  cause_heat, number of existing responses) to limit scope.
- **Trigger threshold**: Amplify on ANY hit, or only when a tension accumulates
  multiple responses? Single-hit amplification gives broader coverage; threshold-
  based is more budget-efficient.

## Next Steps

→ `/workflows:plan` for implementation details
