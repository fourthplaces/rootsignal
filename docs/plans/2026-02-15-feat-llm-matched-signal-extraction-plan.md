---
title: LLM-Matched Signal Extraction
type: feat
date: 2026-02-15
---

# LLM-Matched Signal Extraction

## Overview

Replace the broken fingerprint-based signal dedup with LLM-driven matching folded directly into the extraction step. The LLM sees existing signals (with aliased IDs) alongside page content and decides whether each extracted signal is an UPDATE of something known or a new INSERT. Signals are never deleted — they fade via recency ranking on `broadcasted_at`.

## Problem Statement

The current system computes a SHA-256 fingerprint from LLM-extracted fields (`signal_type + content + entity_name + about`) and uses `ON CONFLICT (fingerprint, schema_version)` to upsert. Because LLM output is non-deterministic ("Open Tues–Sat" vs "Tuesday through Saturday"), fingerprints almost never collide. Every re-extraction creates duplicate signals that pile up indefinitely. The dedup is theater.

## Proposed Solution

### The Entity-Before-Extraction Problem

The core tension: we need existing signals to show the LLM, but we don't know which entities are on a page until the LLM tells us.

**Solution: URL-based lookup.** Before extraction, query signals previously extracted from the same URL:

```sql
SELECT DISTINCT s.entity_id FROM signals s
JOIN page_snapshots ps ON ps.id = s.page_snapshot_id
WHERE ps.url = $1 AND s.entity_id IS NOT NULL
```

Then fetch existing signals for those entities. First-time pages get no existing signals (all INSERTs). This is imperfect but simple and correct for the re-extraction case, which is where dedup matters.

For entity-less signals, also include signals directly linked to the same URL:

```sql
SELECT * FROM signals s
JOIN page_snapshots ps ON ps.id = s.page_snapshot_id
WHERE ps.url = $1 AND s.entity_id IS NULL
ORDER BY broadcasted_at DESC NULLS LAST
LIMIT 20
```

### Extraction Flow (Revised)

1. Fetch snapshot content (as today)
2. **NEW**: Query entity IDs previously associated with this URL
3. **NEW**: Fetch existing signals for those entities + entity-less signals for this URL (cap: 50 signals, ordered by `broadcasted_at DESC`)
4. **NEW**: Build alias map: `signal_1 → uuid-abc, signal_2 → uuid-def, ...`
5. **NEW**: Format existing signals into prompt context section
6. Call LLM with page content + existing signals context
7. For each extracted signal:
   - If `existing_signal_alias` is set AND alias exists in map → **UPDATE**
   - Otherwise → **INSERT**
8. Resolve entity, create extraction record, normalize (location/schedule/embedding) — same as today

### UPDATE Semantics

On UPDATE, overwrite all mutable fields with the fresh extraction:
- `content`, `about`, `signal_type`, `confidence`
- `broadcasted_at` (COALESCE — keep old if new is NULL)
- `page_snapshot_id`, `extraction_id` (link to latest)
- `source_url`, `updated_at`

For polymorphic records (location, schedule, embedding): **delete and recreate**. These are derived data, not user-authored. The signal ID is the stable anchor; everything hanging off it is refreshable.

### Alias Safety

- LLM never sees real UUIDs — only `signal_1`, `signal_2`, etc.
- If alias doesn't exist in map → treat as INSERT (hallucination guard)
- If alias is valid but LLM mismatches → accept the risk. The old content is overwritten but not lost (preserved in the `extractions` table data payload). A future enhancement could add embedding similarity validation before applying UPDATEs.

### Signal Lifecycle

- **INSERT**: New fact, not seen before
- **UPDATE**: Re-observed fact — refresh content, bump timestamps
- **Fade**: Signals not re-confirmed naturally sink in recency ranking
- **No deletion**: Signals persist as historical record

## Acceptance Criteria

- [x] Existing signals for known entities are included in the LLM extraction prompt
- [x] LLM can return an alias to indicate "this updates an existing signal"
- [x] Valid aliases trigger UPDATE; invalid/missing aliases trigger INSERT
- [x] Fingerprint hashing logic removed from extraction activity
- [x] `Signal::create` replaced with explicit INSERT and UPDATE paths
- [x] `fingerprint` column made nullable, unique constraint dropped (migration)
- [x] `ExtractedSignal` struct has new `existing_signal_alias: Option<String>` field
- [x] Extraction prompt updated with existing signals context and matching instructions
- [x] Polymorphic records (location, schedule, embedding) are refreshed on UPDATE
- [x] First-time extractions (no prior signals for URL) still work correctly (all INSERTs)

## MVP

### Phase 1: Schema & Model Changes

#### Migration: `migrations/057_drop_signal_fingerprint_constraint.sql`

```sql
-- Drop the fingerprint unique constraint (dedup is now LLM-driven)
ALTER TABLE signals DROP CONSTRAINT IF EXISTS signals_fingerprint_schema_version_key;
ALTER TABLE signals ALTER COLUMN fingerprint DROP NOT NULL;

-- Drop extraction fingerprint constraint too
ALTER TABLE extractions DROP CONSTRAINT IF EXISTS extractions_fingerprint_schema_version_key;
ALTER TABLE extractions ALTER COLUMN fingerprint DROP NOT NULL;
```

#### `modules/rootsignal-core/src/types.rs`

Add alias field to `ExtractedSignal`:

```rust
pub struct ExtractedSignal {
    // ... existing fields ...

    /// If this signal updates a previously known signal, set this to the
    /// alias provided in the prompt context (e.g. "signal_3").
    /// Leave null if this is a new signal.
    pub existing_signal_alias: Option<String>,
}
```

#### `modules/rootsignal-domains/src/signals/models/signal.rs`

Add new methods:

```rust
impl Signal {
    /// Fetch signals previously extracted from the same URL.
    pub async fn find_by_url(url: &str, limit: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT s.* FROM signals s
            JOIN page_snapshots ps ON ps.id = s.page_snapshot_id
            WHERE ps.url = $1
            ORDER BY s.broadcasted_at DESC NULLS LAST, s.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(url)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Insert a new signal (no fingerprint, no upsert).
    pub async fn insert(...) -> Result<Self> { /* plain INSERT RETURNING * */ }

    /// Update an existing signal with fresh extraction data.
    pub async fn update_from_extraction(
        id: Uuid,
        signal_type: &str,
        content: &str,
        about: Option<&str>,
        entity_id: Option<Uuid>,
        source_url: Option<&str>,
        page_snapshot_id: Option<Uuid>,
        extraction_id: Option<Uuid>,
        confidence: f32,
        broadcasted_at: Option<DateTime<Utc>>,
        pool: &PgPool,
    ) -> Result<Self> {
        /* UPDATE ... SET content=$2, ...
           broadcasted_at = COALESCE($10, signals.broadcasted_at),
           updated_at = NOW()
           WHERE id = $1 RETURNING * */
    }
}
```

### Phase 2: Prompt Changes

#### `config/prompts/signal_extraction.md`

Add a new section (injected dynamically when existing signals are available):

```markdown
## Previously Known Signals

The following signals have been previously extracted for entities on this page.
For each signal you extract, check if it updates a previously known signal.
If it does, set `existing_signal_alias` to the alias (e.g. "signal_3").
If it's genuinely new information, leave `existing_signal_alias` null.

A signal is an UPDATE when:
- It describes the same real-world fact, even if worded differently
- Hours changed, dates shifted, details updated — same underlying thing

A signal is NEW when:
- It describes a different fact (different event, different offer, different need)
- It's about a different entity than any existing signal

{existing_signals_context}
```

The `{existing_signals_context}` block is formatted as:

```
signal_1: [give] "Free meals every Tuesday, no questions asked" (about: food assistance) — 2026-01-15
signal_2: [event] "Community meeting Thursday to discuss the proposed development" (about: community development) — 2026-01-20
```

### Phase 3: Extraction Activity Changes

#### `modules/rootsignal-domains/src/signals/activities/extract_signals.rs`

Modify `extract_signals_from_snapshot`:

1. After fetching snapshot, query existing signals via `Signal::find_by_url(snapshot.url, 50, pool)`
2. Build alias map: `HashMap<String, Uuid>` mapping `"signal_1" → uuid`, etc.
3. Format existing signals into prompt context string
4. Append context to user prompt (or inject into system prompt)
5. In the signal processing loop:
   - Check `signal.existing_signal_alias`
   - If alias exists in map → call `Signal::update_from_extraction(map[alias], ...)`
   - If alias missing/invalid → call `Signal::insert(...)`
6. On UPDATE: delete existing locationables, schedules, embeddings for the signal ID before recreating
7. Remove fingerprint hashing (delete the SHA-256 block entirely)
8. Remove `sha2` import if no longer needed

### Phase 4: Extraction Record Changes

#### `modules/rootsignal-domains/src/signals/activities/extract_signals.rs`

For the `extractions` table insert:
- Generate a random fingerprint (or use snapshot_id + index as a deterministic key) since the unique constraint is dropped
- Or: switch to a plain INSERT without ON CONFLICT

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Entity lookup before extraction | URL-based (prior signals for same URL) | Simple, correct for re-extraction. First-time pages degrade gracefully to all-INSERT |
| Alias format | `signal_N` sequential | Simple, unambiguous, easy to validate |
| UPDATE scope | Full replacement of mutable fields | Simpler than partial patches. Old data preserved in extractions table |
| Polymorphic records on UPDATE | Delete and recreate | They're derived data. Signal ID is the stable anchor |
| Fingerprint column | Make nullable, drop constraint | Backward compatible. Can be fully removed later |
| Concurrency | Last-write-wins, no locking | Matches "signals never deleted" philosophy. Duplicates fade naturally |
| Max existing signals in prompt | 50, ordered by broadcasted_at DESC | Balances matching quality vs token budget |
| LLM mismatch risk | Accept, rely on extractions audit trail | Low probability, non-destructive (overwrite not delete), future embedding validation possible |

## References

- Brainstorm: `docs/brainstorms/2026-02-15-signal-refresh-brainstorm.md`
- Current extraction: `modules/rootsignal-domains/src/signals/activities/extract_signals.rs`
- Signal model: `modules/rootsignal-domains/src/signals/models/signal.rs`
- ExtractedSignal type: `modules/rootsignal-core/src/types.rs:164-204`
- Extraction prompt: `config/prompts/signal_extraction.md`
- Restate workflow: `modules/rootsignal-domains/src/extraction/restate/mod.rs`
- Signal schema: `migrations/049_signals.sql`
