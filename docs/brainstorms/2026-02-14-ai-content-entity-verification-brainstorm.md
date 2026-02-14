---
date: 2026-02-14
topic: ai-content-entity-verification
---

# AI-Generated Content & Entity Verification

## What We're Building

A unified observations system that replaces `signal_enrichments` and introduces agentic entity investigation. The system answers "what do we know about this thing, and how did we learn it?" for any subject in Taproot — signals, organizations, sources, or submitters.

## Why This Matters

AI-generated content is making it cheaper to produce fake signals at scale — fabricated GoFundMes, fake org websites, bogus event listings. Taproot's architecture has natural defenses (action URLs link back to source, multi-source verification, confidence scoring), but there's no investigation layer for the entities producing signal. The current `organizations` table has a binary `verified` boolean with no definition of what verified means.

## Key Decisions

### 1. Taproot's natural position is strong but incomplete

As a signal concentrator (not a content platform), Taproot is less vulnerable to AI content than social platforms. Upstream platforms bear the primary fraud surface. But source entities still need verification — a fake nonprofit website gets scraped just like a real one.

### 2. Entity confidence is computed from multiple corroborating signals, not a binary flag

Replace the `verified` boolean on organizations with a computed confidence derived from structured observations: domain age, IRS tax-exempt status, cross-platform presence, historical signal density, address verification, web content analysis.

### 3. Investigations are agentic

An investigation agent gets dispatched with a subject and a set of tools (WHOIS lookup, IRS database, cross-platform search, etc.). It runs the tools, evaluates results, and writes structured observations back to the database.

### 4. Observations are polymorphic, not entity-specific

Findings attach to any subject type via `subject_type + subject_id`. This avoids parallel tables for signals vs. orgs vs. sources.

### 5. Investigations include a plain-english summary

Each observation stays purely structured and computable. The investigation record includes a `summary` field — the agent's plain-english synthesis of what it found and why the confidence score is what it is. One is for machines, the other is for the human who asks "why is this entity flagged?"

### 6. `signal_enrichments` is absorbed into the unified observations table

Automated pipeline enrichments (freshness, capacity, urgency from Tier 2 scraping) and agent-driven investigation findings are structurally identical — typed observations with jsonb values, confidence, source, and timestamp. They differ only in workflow, which is metadata, not structure. One table handles both.

## Schema

```sql
-- Replaces signal_enrichments. Unified "what we know about things."
observations
├── id (uuid)
├── subject_type (text)        -- "signal", "organization", "source", "submitter"
├── subject_id (uuid)
├── observation_type (text)    -- domain_age, irs_status, freshness, capacity,
│                                 urgency, sentiment, event_status,
│                                 platform_presence, address_verification,
│                                 web_content_analysis, signal_history, ...
├── value (jsonb)              -- structured result data
├── source (text)              -- "whois", "irs_api", "instagram_scrape", ...
├── confidence (float)
├── observed_at (timestamptz)
└── investigation_id (uuid, nullable)  -- null for automated pipeline

-- Tracks investigation runs (both automated and agent-driven)
investigations
├── id (uuid)
├── subject_type (text)
├── subject_id (uuid)
├── trigger (text)             -- "scrape_pipeline", "new_entity", "scheduled", "manual"
├── status (enum)              -- pending, running, completed, failed
├── started_at (timestamptz)
├── completed_at (timestamptz)
├── summary_confidence (float) -- rolled up from individual observations
└── summary (text)             -- agent's plain-english synthesis of findings
```

### What changes on existing tables

- `signal_enrichments` — removed, absorbed into `observations`
- `signals.freshness_score`, `signals.capacity_status` — still exist as computed/cached fields, now derived from `observations WHERE subject_type = 'signal'`
- `organizations.verified` — replaced by `investigations.summary_confidence` for the org

### Investigation agent tools (initial set)

- WHOIS domain age lookup
- IRS tax-exempt database query
- State business registration lookup
- Cross-platform presence search
- Physical address geocode verification
- Web content quality analysis (stock photos, thin content, template detection)
- Taproot signal history query (internal — how long have we seen this entity?)

### Tier boundary preserved

Observations sourced from Tier 2 platforms never leak into consumer-facing API responses. This is enforced at the API layer, same as before — the storage is unified but the serving boundary remains structural.

## Open Questions

- Should automated pipeline enrichments also create `investigation` records for full traceability, or is `investigation_id = null` sufficient?
- What triggers an investigation? Every new org? Confidence dropping below a threshold? Manual only at first?
- How often should investigations be re-run for existing entities?

## Next Steps

- Update `signal-service-architecture.md` to reflect the unified observations model
- Design the investigation agent's tool interface
- Define the observation_type enum more precisely
