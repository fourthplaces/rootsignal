---
date: 2026-02-14
topic: multi-language-support
---

# Multi-Language Support

## What We're Building

A decoupled translation and cross-language search layer for Taproot. Content scraped in any supported language is preserved in its original form, translated into all supported locales, and made discoverable via cross-language semantic search. English serves as the canonical embedding language — all vector search operates in a shared English vector space regardless of source language.

Supported locales: English (`en`), Spanish (`es`), Somali (`so`), Haitian Creole (`ht`).

## Why This Approach

Three concerns that are often conflated — content storage, translation, and search — are kept in separate tables with separate responsibilities:

- **`translations`** — stores localized text per field per record. Purely a text concern.
- **`embeddings`** — stores one vector per locale per record, composed from meaningful fields. Purely a search concern.
- **`source_locale`** on source records — tracks what language the content was originally in.

This separation means each concern can evolve independently: swap translation providers, add languages, change embedding models, or adjust which fields compose an embedding — without touching the others.

## Key Decisions

- **Translation is decoupled from extraction**: The AI extraction step focuses on structuring signal. Translation is a separate pipeline step with its own concern.
- **English translation is infrastructure, not just presentation**: For non-English content, translation to English must happen before embedding. English is the lingua franca of the vector space.
- **Embeddings get their own table**: The `translations` table is per-field (title, description, etc.) but embeddings are per-record. A separate `embeddings` table avoids embedding every translated property and provides a single source of truth for vectors — replacing the current `embedding` column on `listings`.
- **Source language detection is hybrid**: AI detects language during extraction, with source-level config as fallback. A Somali org can still post in English.
- **Four languages to start**: `en`, `es`, `so`, `ht` — driven by actual community need.

## Pipeline

```
Scrape → Extract (detect source_locale)
  → if not English: translate to English (priority, blocking)
  → Embed (always from English text)
  → Translate to remaining languages (async, decoupled)
```

## Schema Shape

### `translations` table

| column           | type      | note                                          |
|------------------|-----------|-----------------------------------------------|
| id               | uuid      |                                               |
| translatable_type| text      | listing, entity, service, tag                 |
| translatable_id  | uuid      |                                               |
| field_name       | text      | title, description, eligibility_description…  |
| locale           | text      | en, es, so, ht                                |
| content          | text      |                                               |
| source_locale    | text      | what language this was translated from         |
| translated_by    | text      | model/provider info                           |
| created_at       | timestamptz |                                             |

### `embeddings` table

| column           | type         | note                                        |
|------------------|--------------|---------------------------------------------|
| id               | uuid         |                                             |
| embeddable_type  | text         | listing, entity, service                    |
| embeddable_id    | uuid         |                                             |
| locale           | text         | en, es, so, ht                              |
| embedding        | vector(1536) |                                             |
| source_text_hash | text         | to know when re-embedding is needed         |
| created_at       | timestamptz  |                                             |

### Changes to existing tables

- Add `source_locale` column to `listings`, `entities`, `services`
- Remove `embedding` column from `listings` (moved to `embeddings` table)

## Translatable Fields

| Record Type | Fields                                                              |
|-------------|---------------------------------------------------------------------|
| Listing     | title, description                                                  |
| Entity      | description                                                         |
| Service     | name, description, eligibility_description, fees_description, application_process |
| Tag         | display_name                                                        |

## Open Questions for Planning

- Should the translation step be a Restate workflow (durable, retryable)?
- How does the API surface translations? (`Accept-Language` header? query param? both?)
- What embedding model composition per record type? (e.g., listing = title + description)
- Unique constraint on `embeddings` — one per (embeddable_type, embeddable_id, locale)?
- How to handle re-translation when source content changes?
- Which translation provider/model for Somali and Haitian Creole specifically?

## Next Steps

→ `/workflows:plan` for implementation details
