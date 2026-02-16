# Phase 1a Implementation Plan — Root Signal (v2)

## Context

Root Signal is a greenfield civic intelligence system. Phase 1a answers one question: **does enough actionable, fresh signal exist in the Twin Cities to make this viable?** (Milestone 1 gate: 100+ actionable signals, 70%+ fresh, 3+ signal types, 3+ audience roles.)

This plan was pressure-tested against all vision docs. v1 had 6 critical safety violations, 3 scope problems, and 4 principle violations. This version fixes them by building safety architecture first and narrowing scope to what Milestone 1 actually requires.

**Phase 1a builds:** Scout agent → graph → web surface → quality measurement. The smallest loop that proves signal exists and is worth surfacing.

**Phase 1a does NOT build:** Investigator agent, response discovery agent, Restate orchestration, human-reported signal, feed view, AI summary, search bar. These move to Phase 1b after Milestone 1 gate passes.

**Why narrower scope:** The kill test warns: "Don't add [extra] until [core] signal is solid." Milestone 1's gate is about signal proof — volume, freshness, diversity, quality. The investigator agent (tracing causal chains, adding Actor/Policy nodes) is Milestone 2+ work. Building it in Phase 1a risks scope creep paralysis while failing to build the measurement infrastructure the gate actually requires.

---

## Crate Structure

```
taproot/
  modules/
    ai-client/                  # EXISTING — LLM client (Claude, OpenAI, OpenRouter)
    apify-client/               # EXISTING — social media scraping
    twilio-rs/                  # EXISTING — not used in 1a
    rootsignal-common/          # NEW — shared types, config, errors, safety types
    rootsignal-graph/           # NEW — Neo4j wrapper, schema, typed CRUD, safety enforcement
    rootsignal-scout/           # NEW — scout agent only (extraction, embedding, dedup)
    rootsignal-web/             # NEW — axum + askama SSR public app (read-only)
```

---

## Safety Architecture

### 1. Sensitivity Classification (Schema-Level)
Every node carries `SensitivityLevel`: General, Elevated, Sensitive. Coordinate precision reduced before data reaches API.

### 2. No Query Logging
Web civic endpoints log only method + route pattern + status + latency. No query params, no cookies, no sessions, no IP logging.

### 3. No Network Graph Exposure
PublicGraphReader exposes only: find_nodes_near, get_node_detail, list_recent. No raw Cypher, no actor traversals.

### 4. Confidence-Tiered Surfacing
- >= 0.6: displayed without disclaimer
- 0.4-0.6: displayed with "limited verification" indicator
- < 0.4: hidden from public, retained in graph

### 5. PII Scrubbing
Extraction prompt strips personal names (non-public-figures), phone numbers, emails, addresses, medical/immigration/financial details. Post-extraction regex validation.

### 6. Expiration + Freshness
Events expire on their date. Fundraisers expire on end date or 60 days. Ongoing signals hidden if not re-confirmed within 30 days.

### 7. Opt-Out Support
`delete_by_source_url(url)` from day one.

---

## Build Order

1. `rootsignal-common` — shared types, safety types, config
2. `rootsignal-graph` — Neo4j wrapper, schema, PublicGraphReader, GraphWriter
3. Extraction + embedding pipeline in rootsignal-scout
4. Scout agent (Tavily + Firecrawl + extraction + dedup + graph writes)
5. Web interface (axum + askama, map + list + detail)
6. Quality measurement dashboard (internal /admin/quality)
7. Pre-launch checklist verification
8. Deploy

---

## Milestone 1 Gate Criteria

- 100+ actionable signals
- 70%+ fresh (within 30 days)
- 3+ signal types
- 3+ audience roles
- Zero PII leaks
- Geo-accuracy > 80%
- Side-by-side vs Google: noticeably better for 7/10 queries
