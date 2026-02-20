---
date: 2026-02-19
topic: story-signal-tags
---

# Story & Signal Tags

## What We're Building

A two-tier tagging system for signals and stories. Signals get LLM-assigned tags during ingestion/enrichment. Story tags are auto-aggregated from constituent signal tags, with admin override for curation. Tags enable precise navigation/filtering and cross-city thematic queries that semantic search handles poorly.

## Why Tags (Kill Test Survived)

Existing mechanisms (category, dominant_type, semantic search, geographic bounds) are insufficient when:
- **High-volume city scrapes** produce dozens of stories where 6 categories are too coarse
- **Cross-city thematic queries** need precision — "ICE" is ambiguous in vector space but unambiguous as a tag `ice-enforcement`
- **Faceted discovery** shows users what exists without requiring them to know what to search for

Tags add precision where vectors give recall. They earn their complexity at scale.

## Key Decisions

- **Two-tier model**: Signal tags (LLM-assigned, high volume, no admin UI) bubble up to story tags (aggregated + admin-curated)
- **Hybrid vocabulary**: LLM suggests freely, tags normalized (lowercased, slugified). LLM receives existing tag vocabulary as context to prefer reuse over invention. Admins can merge/rename at story level.
- **LLM auto-tags, admin overrides**: Consistent with existing pattern (category is already LLM-assigned). Zero admin effort by default, full control when needed.
- **Category stays separate**: Coarse editorial bucket (Crisis, Governance, etc.) remains distinct from granular tags. May converge later — YAGNI for now.
- **Tag-as-node in Neo4j**: `(:Tag {slug, label})<-[:TAGGED]-(Story|Signal)` — supports merge operations (repoint edges) and graph-native queries.

## Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| Tag explosion (40+ tags per story) | Aggregation logic: frequency threshold or LLM consolidation at story level |
| Tag drift / synonyms | Pass existing vocabulary to LLM during tagging; admin merge tools |
| Retroactivity on merge | Tag-as-node model makes merge = repoint edges |
| Eventual consistency (signal tags lag on stories) | Acceptable — same as velocity/energy refresh in Phase B |
| Budget exhaustion (no LLM = no tags) | Graceful degradation, same pattern as synthesis |

## Open Questions

- Tag aggregation at story level: frequency threshold (N+ signals) vs. LLM consolidation vs. both?
- Multi-tag intersection queries in the API (AND/OR semantics)?
- UI: tag chips on StoryCard, filterable sidebar, tag cloud?
- Should signal tagging happen during initial ingestion or during a separate enrichment pass?

## Next Steps

-> `/workflows:plan` for implementation details
