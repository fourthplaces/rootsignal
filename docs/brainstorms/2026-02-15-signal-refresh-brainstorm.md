---
date: 2026-02-15
topic: signal-refresh-strategy
---

# Signal Refresh Strategy

## What We're Building

Replace the current fingerprint-based signal dedup (which doesn't work because LLM output is non-deterministic) with an LLM-driven matching approach folded directly into the extraction step. Signals are never deleted — they naturally fade via recency ranking.

## Problem

The current system computes a SHA256 fingerprint from LLM-extracted fields (signal_type, content, entity_name, about) and uses `ON CONFLICT` to upsert. In practice, LLM output varies enough between runs that fingerprints almost never collide, so every re-extraction creates duplicate signals that pile up indefinitely.

## Chosen Approach: LLM-Matched Extraction with Aliased IDs

### How It Works

1. Before extraction, fetch existing signals for the entity (or entities on the page)
2. Assign simple aliases: `signal_1`, `signal_2`, etc. mapped to real UUIDs
3. Pass existing signals with aliases as context to the LLM alongside the page content
4. LLM extracts signals and for each one either:
   - References an existing alias (e.g. `signal_3`) → **UPDATE** that signal
   - Marks it as new → **INSERT** a new signal
5. Validate aliases before applying: if alias doesn't exist in the map, treat as INSERT

### Why This Approach

- **LLM understands identity, not just similarity.** It knows "Open Tues–Sat" and "Tuesday through Saturday" are the same signal, and "Tuesday jazz night" vs "Thursday open mic" are different — something embedding similarity and fingerprints can't reliably distinguish.
- **Aliases prevent hallucination risk.** The LLM never sees real UUIDs. If it returns an alias outside the provided set, we know it's wrong and safely treat it as a new signal.
- **Folding matching into extraction is one LLM call**, not extract-then-match as two separate steps.

### Signal Lifecycle

- **INSERT**: New fact not seen before
- **UPDATE**: Re-observed fact — refresh content, `broadcasted_at`, link to new snapshot
- **Fade**: Signals that stop being re-confirmed naturally sink in recency ranking
- **No deletion**: Signals persist as historical record, surfaced or buried by query-time ranking on `broadcasted_at`

## What This Eliminates

- Fingerprint hashing logic (SHA256 of LLM output)
- `ON CONFLICT (fingerprint, schema_version)` upsert pattern
- Any need for staleness/decay mechanisms
- Reconciliation steps, generations, or soft-delete flags

## Key Decisions

- **LLM is the dedup engine**: Matching is a reasoning task, not a hashing task
- **Aliases over real IDs**: Safety by construction — can't corrupt unrelated signals
- **No deletion**: Recency ranking at query time handles currency
- **One LLM call**: Matching folded into extraction, not a separate step

## Open Questions

- How many existing signals per entity before context gets too large? May need to pre-filter by signal_type or recency for entities with many signals
- Exact prompt structure for the extraction+matching call
- Whether `broadcasted_at` alone is sufficient for ranking or if `last_confirmed_at` adds value as a separate field

## Next Steps

→ `/workflows:plan` for implementation details
