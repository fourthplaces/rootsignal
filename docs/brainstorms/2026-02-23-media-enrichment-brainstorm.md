---
date: 2026-02-23
topic: media-enrichment
---

# Media Enrichment: Transcription & Image Text Extraction

## What We're Building

An asynchronous enrichment pass that populates `ArchiveFile.text` for video and image attachments. Videos go through OpenAI Whisper API, images go through Claude vision. The enrichment runs independently from fetch and scout — nothing blocks.

## The Flywheel

**Fetch** → stores posts + files (`text = NULL`) → **Archive kicks off enrichment via Restate** → **Scout Pass 1** works with captions only → **Enrichment completes in background** → **Scout Pass 2** picks up new text → new signals extracted

```
source("instagram.com/handle").posts(20)

→ posts[0]  (newest)  text: "Check out..."  attachments[0].text: NULL     ← just fetched
→ posts[1]            text: "Big turnout"   attachments[0].text: NULL     ← same
→ posts[2]            text: "Spring plans"  attachments[0].text: "We've…" ← enriched previous pass
→ posts[19] (oldest)  text: "First meeting" attachments[0].text: "Hello…" ← enriched long ago
```

## Why This Approach

- **Separate enrichment pass** over at-fetch-time: decouples slow AI calls from fetching, makes both retryable independently
- **NULL is the signal** over explicit status field: YAGNI — scout just works with what's available, no coordination needed
- **Archive owns enrichment, not scout**: scout never knows about transcription/OCR — it just sees `file.text` as populated or not
- **Restate for durability**: enrichment is long-running and can fail (API timeouts, rate limits) — Restate handles retries and replay

## Key Decisions

- **OpenAI Whisper API** for video/audio transcription (purpose-built, reliable, cheap)
- **Claude vision** for image text extraction (ai-client already in workspace)
- **`WorkflowDispatcher` trait** on archive boundary — archive calls `dispatcher.enrich(file_ids)` without knowing about Restate. Production wires in a Restate-backed implementation. Tests mock it. Same pattern as `ContentFetcher`.
- **Archive triggers enrichment on fetch** — when `posts()` or `stories()` detects media attachments with `text = NULL`, it fires off enrichment and returns immediately
- **`EnrichmentWorkflow` in Restate** — durable workflow that queries unenriched files, calls Whisper/Claude vision, updates `file.text`
- **No fixtures needed** for testing — mock the dispatcher and enricher, test the flywheel mechanics

## Architecture

```
Scout                          Archive                        Restate
  │                              │                              │
  │  source("...").posts(20)     │                              │
  │─────────────────────────────►│                              │
  │                              │  fetch from Instagram        │
  │                              │  store posts + files          │
  │                              │  (file.text = NULL)          │
  │                              │                              │
  │                              │  detect media attachments    │
  │                              │  dispatcher.enrich(file_ids) │
  │                              │─────────────────────────────►│
  │                              │                              │  EnrichmentWorkflow
  │  ◄─── returns posts ────────│                              │  (runs in background)
  │  (text = NULL on new files)  │                              │
  │                              │                              │  video → Whisper API
  │  works with captions         │                              │  image → Claude vision
  │                              │                              │  UPDATE file.text
  │                              │                              │
  │  ... next scout pass ...     │                              │
  │  source("...").posts(20)     │                              │
  │─────────────────────────────►│                              │
  │  ◄─── returns posts ────────│                              │
  │  (text now populated!)       │                              │
```

### Trait Boundary

```rust
#[async_trait]
pub trait WorkflowDispatcher: Send + Sync {
    async fn enrich(&self, file_ids: Vec<Uuid>) -> Result<()>;
}

// Production: posts to Restate ingress
// Tests: MockDispatcher that records calls
```

## Testing Strategy

Tests verify the flywheel, not model quality:

1. Mock fetcher returns files with `text = NULL`
2. Mock dispatcher records `enrich()` was called with correct file IDs
3. Simulate enrichment by updating file text directly
4. Assert that on next read, `file.text == Some(...)`
5. No fixtures, no real API calls — fast, deterministic, in-process

Pattern: MOCK → FUNCTION → OUTPUT

## Open Questions

- Does enrichment need to download media bytes, or do the APIs accept URLs directly? (Whisper API accepts file uploads; Claude vision accepts base64 or URLs)
- Should `WorkflowDispatcher` live in `rootsignal-common` or a new shared crate?
- Batch size / rate limiting for enrichment (how many files per workflow invocation)?

## Next Steps

→ `/workflows:plan` for implementation details
