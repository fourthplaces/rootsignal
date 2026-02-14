---
date: 2026-02-14
topic: dissemination-intent
---

# Dissemination Intent: Public Service Announcements via Taxonomy

## What We're Building

A new `dissemination` tag kind that captures **how a listing should reach its audience** — distinguishing passive searchable content from proactive broadcasts and urgent alerts. This enables PSA-like behavior (government advisories, community alerts like ICE watch, neighborhood announcements) without new tables or schema changes.

## Why This Approach

Listings already cover the content model for PSAs: entity authorship, geographic targeting, urgency, temporal bounds, audience tags, and multi-language support. The gap is that the taxonomy describes *what* a listing is and *how urgent* it is, but not the **communication intent** — whether it should be passively available or actively pushed.

A new tag kind is the simplest path. It composes with all existing dimensions and requires only a seed migration.

## Key Decisions

- **Tag kind, not a column or new entity**: PSA behavior is a classification concern, not a structural one. The `tag_kinds` system already supports dynamic taxonomy expansion.
- **Name: `dissemination`** (not `signal_type`): Avoids collision with `listing_type`, which is already described as "Signal Type" in tag_kinds.
- **Four values with clear semantics**:
  - `passive` — available if searched for (implicit default for existing listings)
  - `announcement` — surface proactively in feeds/dashboards
  - `alert` — push notification / urgent broadcast
  - `advisory` — persistent warning, shown until expires_at

## Composition Examples

| Scenario | listing_type | signal_domain | dissemination | urgency |
|---|---|---|---|---|
| ICE sighting | community_alert | community_safety | alert | immediate |
| Park cleanup Saturday | community_event | civic_economic | announcement | this_week |
| Boil water advisory | water_quality_alert | community_safety | advisory | immediate |
| Free flu shots | health_screening | human_services | announcement | this_month |
| Public comment deadline | public_comment_period | civic_economic | announcement | this_week |

## Open Questions

- Should `passive` be an explicit tag value, or should absence of a `dissemination` tag imply passive? (Leaning toward explicit for queryability.)
- How does the consuming app handle `alert` — push notifications, SMS, or just prominent UI placement? (Deferred to app layer design.)
- Should `dissemination` be `required: true` on listings, or optional with passive as default? (Leaning optional for backward compat with existing data.)

## Next Steps

- Seed migration adding `dissemination` tag kind + 4 values
- GraphQL filtering support for `dissemination` queries
