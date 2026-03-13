# Coalescing → Weaving: Layered Architecture

**Date**: 2026-03-11

## Core Insight

Situation weaving conflates two concerns:
1. **Clustering** — "these signals belong together" (analytical, pattern recognition)
2. **Editorial synthesis** — dispatches, narratives, temperature, arc lifecycle (storytelling)

Coalescing should own #1. Weaving should own #2. Clean separation:
- **Groups** = *that* these signals belong together (evidence)
- **Situations** = *what* is happening and *why* it matters (interpretation)

## The Layered Model

```
1. FIND     → scout pipeline creates signals
2. COALESCE → signals cluster into groups (analytical)
3. WEAVE    → groups get narrated into situations (editorial)
```

### Coalescing Layer (Analytical)
- Signal clustering via LLM-driven search (seed + feed modes)
- Temperature computation (urgency/heat of the cluster)
- Arc lifecycle (emerging → developing → cold)
- Curiosity triggers (gaps worth investigating)
- Source boosting (hot groups increase scrape cadence)
- Category classification (dominant theme)
- Centroid computation (geographic center)

### Weaving Layer (Editorial)
- Reads groups, writes situations
- Headline + lede (naming the narrative)
- Dispatches (human-readable summaries with signal citations)
- Structured state (narrative context across weaving rounds)
- Dispatch verification (citation checking, PII detection)

## Future: Hierarchical Groups

Groups can point to groups. Group A tells a story, Group B tells a story,
and A + B together tells a story about how and why they're connected.

```
Now:     Signal → Group → Situation ("what/why")
Later:   Group → Meta-Group → Situation ("how A and B connect")
```

Option 2 from the original analysis — situations spanning multiple groups —
becomes natural AFTER groups-pointing-to-groups exists.

## Drift Analysis

Current situation weaving and coalescing solve the same problem (signal clustering)
with incompatible approaches:

| | Situation Weaving | Coalescing |
|---|---|---|
| Unit of work | Individual signal → assign to Situation | Seed signal → discover cluster via search |
| Discovery | Menu selection (LLM picks from existing) | Emergent (LLM uses search tools) |
| Input scope | Signals from this scout run only | All ungrouped signals across runs |
| Edge type | PART_OF → Situation | MEMBER_OF → SignalGroup |
| Richness | Dispatches, temperature, arc, structured_state | Label + queries only |
| Multi-membership | No | Yes |

Decision: **Option B — evolve coalescing, simplify weaving.** Coalescing's emergent
discovery model is stronger. Weaving's editorial richness is valuable but belongs
on top of groups, not conflated with clustering.

## Migration Phases

### Phase 1: Move Analytical Metadata onto SignalGroup

SignalGroup today: `label`, `queries`, `created_at`.
Needs: `temperature`, `arc`, `cause_heat`, `centroid_lat/lng`, `signal_count`, `category`.

New event: `GroupTemperatureComputed { group_id, temperature, arc }`
or repurpose existing temperature computation targeted at groups.

Existing `situation_temperature.rs` logic lifts nearly verbatim — already computes
from signal properties. Change "signals in this situation" → "signals in this group."

### Phase 2: Move Curiosity Triggers + Source Boost to Coalescing

Currently in `weave_situations()`:
- Source boost: hot situations → `SourcesBoostedForSituation`
- Curiosity: fuzzy situations → `CuriosityTriggered`

These become post-coalescing:
- Source boost: hot groups → boost source cadence
- Curiosity: possibly absorbed into coalescing itself — feed mode's search
  for new signals matching a group's queries IS curiosity investigation

### Phase 3: Implement Coalescer::run()

Now we know what downstream expects:
- Output: groups with signals, labels, queries
- Post-processing: temperature computed on each group
- Downstream: weaving reads groups

Seed mode: highest-heat ungrouped signal, 3 rounds of LLM search.
Feed mode: existing groups' queries pull in new matching signals.

### Phase 4: Simplify Weaving to Read Groups

Input changes: individual signals → groups.
- `discover_unassigned_signals()` → `discover_unnarrated_groups()`
- No more cosine-similarity candidate ranking — coalescer did the clustering
- LLM prompt simplifies: "here's a pre-clustered group, explain what/why"
- Dispatches, headline, lede, structured_state all stay

### Phase 5: Causal Chain

Coalescing fires on `GenerateSituationsRequested`, emits `CoalescingCompleted`.
Weaving fires on `CoalescingCompleted`, reads groups from Neo4j.

## Open Questions

### Not every group needs a situation
Some groups may be too small or too cold. Weaving should filter:
"give me groups with temperature > X or signal_count > Y."
Ungrouped signals and unnarrated groups both surface independently in the API.

### Existing data migration
- Existing Situation nodes stay (editorial layer still valid)
- Existing PART_OF edges become stale (signals bypass groups)
- Options: backfill groups from existing situation membership, or rebuild naturally
- No rush — old data isn't wrong, just missing the intermediate layer

### Curiosity as coalescing, not a separate trigger
Feed mode (searching for signals matching a group's queries) IS curiosity.
When the coalescer finds nothing, that's a gap signal.
When it finds something new, that's corroboration.
Separate `CuriosityTriggered` events may be unnecessary.

### Temperature at group level vs situation level
If situations explain groups 1:1, temperature lives on the group.
If situations span multiple groups (later), situation temperature becomes
an aggregate of group temperatures. Either way, computation lives in coalescing.
