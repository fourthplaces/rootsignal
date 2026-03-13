---
date: 2026-03-13
topic: group-gravity-feed
---

# Group Gravity Feed

## What We're Building

A per-group feed mechanism that treats signal groups as gravitational wells. Each group has stored search queries that define its "gravity" — when fed, the coalescer runs those queries against the database to find new signals that match, then asks the LLM whether they belong. Groups are fed on a self-adjusting schedule: frequent when active, backing off exponentially when they stop attracting new signals.

When a feed finds new signals, it automatically chains a re-weave of the group's situation to update the briefing with the new information.

## Why This Approach

The `feed_single_group()` function already exists in the coalescer — it runs a group's queries, finds candidates, and asks the LLM to judge membership. Today it only runs as part of a full region-wide coalesce (`feed_mode()` iterates up to 5 groups). This approach extracts that into a standalone, per-group workflow with its own lifecycle.

Exponential backoff was chosen over hard decay thresholds because it requires no configuration, no arbitrary cutoffs, and groups naturally cool without anyone deciding when they're "dead." A successful feed resets the interval, so a dormant group that suddenly attracts signals snaps back to life.

## Key Decisions

- **Per-group, not region-wide**: Feed targets one group at a time. No broad sweeps.
- **Manual + scheduled**: "Feed" button on Cluster Detail page for on-demand use. Scheduled feeds run automatically.
- **Auto-chain to re-weave**: Feed completion with new signals triggers a re-weave of the group's woven situation via the existing chain pattern (`parent_run_id`).
- **Exponential backoff on empty feeds**: Base interval (e.g. 1 hour). Doubles each consecutive empty result (1h → 2h → 4h → 8h → ...). Successful feed (new signals added) resets to base interval.
- **Auto-schedule on group creation**: When a new group is created during coalescing, it automatically gets a feed schedule at base interval.
- **No hard death**: Groups never fully stop — they just feed less and less frequently. Remain discoverable and manually feedable.
- **Event-driven chaining**: Feed → re-weave chaining follows the same `parent_run_id` pattern as scout → coalesce → weave. This is the broader pattern for workflow composition.

## Open Questions

- What base interval? 1 hour feels right for active groups, but could be configurable per region.
- Should there be a cap on backoff (e.g. max 7 days) or let it grow unbounded?
- Should a manual "Feed" also reset the schedule interval, or only automatic feeds?

## Next Steps

→ `/workflows:plan` for implementation details
