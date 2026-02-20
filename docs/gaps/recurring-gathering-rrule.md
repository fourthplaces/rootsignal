# Recurring Gathering Schedule Data

**Status:** Deferred (not MVP)

## The Gap

`GatheringNode.is_recurring` is a bare boolean — we know *that* something recurs but not *how*. There is no recurrence pattern, no timezone, and no queryable next-occurrence.

Current struct (`types.rs`):

```rust
pub struct GatheringNode {
    pub meta: NodeMeta,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub action_url: String,
    pub organizer: Option<String>,
    pub is_recurring: bool,
}
```

`starts_at`/`ends_at` are UTC with no original timezone preserved. For recurring events this means we can't answer "when is the next occurrence?" or "what's happening this weekend?" without external logic.

## Proposed Approach: RRULE + Materialized Next Occurrence

### Struct Changes

- Replace `is_recurring: bool` with `rrule: Option<String>` (RFC 5545 recurrence rule)
- Add `timezone: Option<String>` (IANA tz name, e.g. `America/Chicago`)
- Add `series_starts_at: Option<DateTime<Utc>>` for when the series originally began
- Repurpose `starts_at`/`ends_at` as the *next* (or current) occurrence

### Why RRULE

- Standard format — Eventbrite, Google Calendar, iCal all speak it
- Compact — a single string like `RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20260601T000000Z` replaces what would otherwise need multiple fields
- `is_recurring` becomes redundant: if `rrule` is `Some(...)`, it recurs

### Why Materialized Dates Matter

RRULE is a generation rule, not queryable data. Queries like "what gatherings are happening this weekend?" need indexed `starts_at` comparisons, not runtime RRULE expansion. The approach:

- Keep `starts_at`/`ends_at` as indexed Neo4j properties (already have indexes)
- A background refresh job expands the RRULE and advances these fields when the current occurrence passes
- Query: `MATCH (g:Gathering) WHERE g.starts_at >= $from AND g.starts_at <= $to`

### Alternatives Considered

- **Exploded occurrence nodes:** each recurrence gets its own node. Fully queryable but creates many nodes (52/year for weekly) and risks stale/orphaned data on RRULE changes.
- **Rolling window expansion:** materialize occurrences for the next N days as linked nodes. More queryable than single next-occurrence but more complex to maintain.

Single materialized next-occurrence (the proposed approach) is simplest and sufficient unless multi-occurrence calendar views are needed.

## Open Questions

- **Extraction fidelity:** Can the LLM extractor reliably produce valid RRULE strings from free-text like "every other Thursday" or "first Saturday of the month"?
- **Refresh cadence:** How often does the background job need to run? Daily is likely sufficient for most use cases.
- **Rust RRULE crate:** Need to evaluate available crates for RRULE expansion (e.g. `rrule-rs`).

## Why Deferred

- Not MVP-critical — most initial sources are one-off or have explicit dates
- RRULE extraction from free-text needs validation before committing to the schema
- Requires background job infrastructure for refreshing materialized dates
