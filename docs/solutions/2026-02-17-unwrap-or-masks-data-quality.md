---
date: 2026-02-17
tags: [data-quality, anti-pattern, extraction, rust]
category: architecture
module: rootsignal-scout
symptoms:
  - All events appear fresh despite stale source content
  - Freshness metrics show no variation
  - Quality scores don't distinguish between complete and incomplete extraction
---

# unwrap_or(default) Masks Data Quality Problems

## Problem

`EventNode.starts_at` was `DateTime<Utc>` (non-optional). When the LLM couldn't parse a date from web content, the extractor used `unwrap_or(now)` — defaulting to the extraction timestamp.

This meant 99% of events had `starts_at = now`, making every event look "happening today" regardless of actual content. The freshness reaper, display filters, and quality scorer all operated on fabricated data.

## Root Cause

Using `unwrap_or(sensible_default)` on LLM extraction output silently converts "I don't know" into "I'm sure it's X." The default value is indistinguishable from a real extracted value downstream.

## Fix

1. Changed `starts_at` from `DateTime<Utc>` to `Option<DateTime<Utc>>`
2. Events with no parseable date get `starts_at: None`
3. Quality scorer rewards `has_timing = starts_at.is_some()`
4. Reaper and display filters skip date-based expiry when `starts_at` is `None` (fall through to general `last_confirmed_active` freshness)
5. Improved LLM prompt: passes today's date, instructs relative date resolution

## Lesson

For LLM-extracted fields: prefer `Option<T>` over `T` with a default. The type system should distinguish "extracted successfully" from "extraction failed." Reserve `unwrap_or` for fields where the default is genuinely correct (e.g., `is_recurring: false`), not for fields where the default fabricates data.

## Files Changed

- `modules/rootsignal-common/src/types.rs` — `EventNode.starts_at: Option<DateTime<Utc>>`
- `modules/rootsignal-scout/src/extractor.rs` — removed `unwrap_or(now)`, improved timing prompt
- `modules/rootsignal-scout/src/quality.rs` — `has_real_timing = e.starts_at.is_some()`
- `modules/rootsignal-graph/src/writer.rs` — handle optional starts_at, fix reap query
- `modules/rootsignal-graph/src/reader.rs` — optional parsing, display filter, expiry clause
