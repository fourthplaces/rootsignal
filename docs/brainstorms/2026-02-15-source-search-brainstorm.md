---
date: 2026-02-15
topic: source-search
---

# Source Search via Website Summary Embeddings

## What We're Building

Semantic search across sources based on AI-generated summaries of their scraped content. During qualification, the AI already reads sample pages — we'll extend it to also produce a ~200-word content summary of what the website covers. That summary gets embedded and stored in the `embeddings` table (polymorphic reference to source). The admin `/sources` page gets a search box that filters sources by semantic similarity.

## Why This Approach

- **Reuses existing infrastructure**: pgvector, embeddings table, OpenAI embedding service, HNSW indexes
- **Minimal pipeline change**: qualification already reads pages and calls AI — just expand the output
- **One vector per source**: trivially small storage footprint
- **Extensible**: per-snapshot embeddings can be layered on later if needed

## Key Decisions

- **Website-level, not page-level**: Start with one summary per source, not per snapshot. Simpler, cheaper, sufficient for source discovery.
- **Polymorphic embeddings table**: Use existing `embeddings` table with reference to source, not new columns on `sources`. Keeps embedding queries explicit and consistent.
- **Generated during qualification**: Summary and embedding created/updated when qualification runs. Re-qualification refreshes the summary.
- **Admin UI integration**: Search box on `/sources` page, filtering by semantic similarity against source embeddings.

## Open Questions

- Exact polymorphic key format for sources in embeddings table (check existing pattern for listings)
- Whether to combine semantic search with existing source filters (entity, type, status) or keep it standalone

## Next Steps

→ Plan implementation details
