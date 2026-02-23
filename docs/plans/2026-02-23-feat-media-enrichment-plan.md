---
title: "feat: Media Enrichment (Whisper + Claude Vision)"
type: feat
date: 2026-02-23
---

# Media Enrichment: Transcription & Image Text Extraction

## Overview

Archive automatically enriches media attachments with extracted text. Videos/audio go through OpenAI Whisper API, images go through Claude vision. Enrichment runs asynchronously via Restate — scout never knows about it. `file.text = NULL` means "not yet enriched," `file.text = ""` means "enriched, nothing found."

## Key Decisions

- **Automatic**: archive always dispatches enrichment for media files with `text = NULL`
- **NULL vs empty string**: `NULL` = not enriched, `""` = enriched but no text found (prevents infinite re-dispatch)
- **Download bytes at dispatch**: archive downloads media bytes before dispatching to Restate (CDN URLs expire)
- **`WorkflowDispatcher` trait**: archive calls a trait, doesn't know about Restate
- **No budget cap in v1**: monitor spend, add controls later if needed
- **`with_text_analysis()` is dead code**: remove it — enrichment is always automatic

## Mime Type Routing

| Pattern | API | Notes |
|---------|-----|-------|
| `image/*` (except `image/svg+xml`) | Claude vision | OCR focus: extract visible text |
| `video/*` | OpenAI Whisper | Extract audio track, transcribe |
| `audio/*` | OpenAI Whisper | Direct transcription |
| Everything else | Skip | No enrichment |

## Architecture

```
Archive fetch
  │
  ├── store posts + files (text = NULL)
  ├── download media bytes for unenriched files
  ├── dispatcher.enrich(Vec<EnrichmentJob>) ── fire and forget
  │                                               │
  └── return posts to caller immediately          ▼
                                            Restate EnrichmentWorkflow
                                              ├── image? → Claude vision
                                              ├── video? → Whisper API
                                              └── UPDATE file.text
```

### Trait Boundary

```rust
// modules/rootsignal-archive/src/enrichment.rs

pub struct EnrichmentJob {
    pub file_id: Uuid,
    pub mime_type: String,
    pub media_bytes: Vec<u8>,
}

#[async_trait]
pub trait WorkflowDispatcher: Send + Sync {
    async fn enrich(&self, jobs: Vec<EnrichmentJob>) -> Result<()>;
}
```

Archive holds `Option<Arc<dyn WorkflowDispatcher>>`. When `None`, enrichment is disabled (tests, offline mode).

### Restate Implementation

```rust
// modules/rootsignal-archive/src/workflows/enrichment.rs

#[restate_sdk::workflow]
#[name = "EnrichmentWorkflow"]
pub trait EnrichmentWorkflow {
    async fn run(req: EnrichmentRequest) -> Result<EnrichmentResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}
```

- Workflow key: hash of sorted file IDs (idempotent — duplicate dispatches are no-ops)
- Each file processed in a `ctx.run()` journaled block (crash-safe per file)
- On permanent failure (retries exhausted): set `file.text = ""` to mark as attempted

## Phases

### Phase 1: Trait + Enrichment Trigger in Archive

**Files:**
- `modules/rootsignal-archive/src/enrichment.rs` — new: `EnrichmentJob`, `WorkflowDispatcher` trait, mime routing helper
- `modules/rootsignal-archive/src/archive.rs` — add `Option<Arc<dyn WorkflowDispatcher>>` to `ArchiveInner`, builder method
- `modules/rootsignal-archive/src/source_handle.rs` — after `upsert_file()`, detect unenriched media files, download bytes, call dispatcher
- `modules/rootsignal-archive/src/source_handle.rs` — remove `with_text_analysis()` from all request builders
- `modules/rootsignal-archive/src/lib.rs` — export enrichment types

**Acceptance criteria:**
- [x] `WorkflowDispatcher` trait defined with `enrich(Vec<EnrichmentJob>)` method
- [x] Archive detects files where `text.is_none()` and mime matches enrichment types
- [x] Archive downloads media bytes from file URL
- [x] Archive calls `dispatcher.enrich()` fire-and-forget (ignores errors)
- [x] `with_text_analysis()` removed from all request builders
- [x] When dispatcher is `None`, no enrichment happens (silent skip)

### Phase 2: Claude Vision Support in ai-client

**Files:**
- `modules/ai-client/src/claude/types.rs` — add `Image` variant to `ContentBlock`, add `ImageSource` struct
- `modules/ai-client/src/claude/mod.rs` — add `describe_image(bytes, mime_type, prompt)` method

**Acceptance criteria:**
- [x] `ContentBlock::Image { source: ImageSource }` variant added
- [x] `ImageSource` struct with `type`, `media_type`, `data` (base64) fields
- [x] `Claude::describe_image()` sends vision request, returns extracted text
- [ ] OCR-focused prompt: "Extract all visible text from this image. Return only the text, nothing else. If no text is visible, return an empty string."

### Phase 3: OpenAI Whisper Client

**Files:**
- `modules/ai-client/src/openai/mod.rs` — new: Whisper client
- `modules/ai-client/src/openai/types.rs` — new: request/response types
- `modules/ai-client/src/lib.rs` — export openai module

**Acceptance criteria:**
- [x] `OpenAi::transcribe(bytes, mime_type)` added (uses existing OpenAi client, no separate Whisper struct needed)
- [x] Sends multipart form to `api.openai.com/v1/audio/transcriptions` with whisper-1 model
- [x] Returns transcribed text as `String`
- [x] Handles video by passing bytes directly (Whisper extracts audio from mp4/webm)

### Phase 4: Restate EnrichmentWorkflow

**Files:**
- `modules/rootsignal-archive/src/workflows/enrichment.rs` — new: workflow definition + impl
- `modules/rootsignal-archive/src/workflows/types.rs` — new: `EnrichmentRequest`, `EnrichmentResult`
- `modules/rootsignal-archive/src/workflows/mod.rs` — new: module root, `impl_restate_serde!` macro, `ArchiveDeps`
- `modules/rootsignal-archive/Cargo.toml` — add `restate-sdk` dependency

**Acceptance criteria:**
- [ ] `EnrichmentWorkflow` defined in archive, following the same Restate pattern (deps injection, status tracking)
- [ ] `ArchiveDeps` holds what enrichment needs: `PgPool`, OpenAI API key, Anthropic API key
- [ ] Each file processed in its own `ctx.run()` block
- [ ] Routes by mime type: image → Claude vision, video/audio → Whisper
- [ ] Updates `store.update_file_text(file_id, text, language)` per file
- [ ] On permanent failure: sets `file.text = ""` (marks as attempted, prevents re-dispatch)
- [ ] Workflow key derived from sorted file IDs (idempotent)

### Phase 5: Wire Production Dispatcher + Register Workflow

**Files:**
- `modules/rootsignal-archive/src/enrichment.rs` — `RestateDispatcher` struct implementing `WorkflowDispatcher` (POSTs to Restate ingress)
- `modules/rootsignal-archive/src/archive.rs` — accept optional dispatcher in `Archive::new()`
- `modules/rootsignal-api/src/main.rs` — construct `RestateDispatcher`, pass to Archive, bind `EnrichmentWorkflowImpl` to Restate endpoint

**Acceptance criteria:**
- [ ] `RestateDispatcher` implements `WorkflowDispatcher` (lives in archive, thin HTTP client)
- [ ] Archive constructed with dispatcher in production
- [ ] `EnrichmentWorkflowImpl` registered on the Restate endpoint in API startup
- [ ] End-to-end: fetch posts → enrichment dispatched → Restate workflow runs → file.text updated

### Phase 6: Tests

Two testing surfaces with different strategies:

#### 6a. Pure logic tests (in `rootsignal-archive`, no Postgres)

The mime-type filtering and dispatch decision logic is extracted as a pure function:

```rust
// modules/rootsignal-archive/src/enrichment.rs

/// Given a list of files, return those needing enrichment.
/// A file needs enrichment when:
///   - text is None (not yet processed — empty string means already attempted)
///   - mime_type matches image/* (except svg), video/*, or audio/*
pub fn files_needing_enrichment(files: &[ArchiveFile]) -> Vec<Uuid> { ... }
```

**Files:**
- `modules/rootsignal-archive/src/enrichment.rs` — pure function + unit tests

**Test cases (MOCK → FUNCTION → OUTPUT):**

```rust
// image_file_with_null_text_needs_enrichment
// MOCK: vec![ArchiveFile { mime_type: "image/jpeg", text: None }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![file.id]

// video_file_with_null_text_needs_enrichment
// MOCK: vec![ArchiveFile { mime_type: "video/mp4", text: None }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![file.id]

// audio_file_with_null_text_needs_enrichment
// MOCK: vec![ArchiveFile { mime_type: "audio/mpeg", text: None }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![file.id]

// already_enriched_file_is_skipped
// MOCK: vec![ArchiveFile { mime_type: "image/jpeg", text: Some("hello") }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![] (empty)

// empty_string_text_means_already_attempted
// MOCK: vec![ArchiveFile { mime_type: "image/jpeg", text: Some("") }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![] (empty — "" means enrichment ran, found nothing)

// pdf_file_is_not_enriched
// MOCK: vec![ArchiveFile { mime_type: "application/pdf", text: None }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![] (not a media type)

// svg_file_is_not_enriched
// MOCK: vec![ArchiveFile { mime_type: "image/svg+xml", text: None }]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![] (SVG excluded)

// mixed_files_returns_only_unenriched_media
// MOCK: vec![
//   ArchiveFile { mime: "image/jpeg", text: None },      ← needs enrichment
//   ArchiveFile { mime: "image/png", text: Some("hi") },  ← already done
//   ArchiveFile { mime: "video/mp4", text: None },        ← needs enrichment
//   ArchiveFile { mime: "application/pdf", text: None },  ← wrong type
// ]
// FUNCTION: files_needing_enrichment(&files)
// OUTPUT: returns vec![jpeg_id, mp4_id]
```

#### 6b. MockDispatcher (in `rootsignal-archive`)

**Files:**
- `modules/rootsignal-archive/src/enrichment.rs` — `MockDispatcher` impl + integration tests

**MockDispatcher** records calls (same pattern as MockSignalStore):

```rust
pub struct MockDispatcher {
    calls: Mutex<Vec<Vec<EnrichmentJob>>>,
}

impl MockDispatcher {
    pub fn new() -> Self { ... }
    pub fn calls(&self) -> Vec<Vec<EnrichmentJob>> { ... }
    pub fn total_files_dispatched(&self) -> usize { ... }
}

#[async_trait]
impl WorkflowDispatcher for MockDispatcher {
    async fn enrich(&self, jobs: Vec<EnrichmentJob>) -> Result<()> {
        self.calls.lock().unwrap().push(jobs);
        Ok(())
    }
}
```

**Integration tests** (archive with MockDispatcher, requires Postgres via testcontainers):

```rust
// archive_dispatches_enrichment_for_media_files
// MOCK: real Archive with MockDispatcher + Postgres
// FUNCTION: archive.source("instagram.com/handle").posts(20).await
// OUTPUT: MockDispatcher.calls() contains the file IDs of unenriched media

// archive_does_not_dispatch_when_no_dispatcher_configured
// MOCK: Archive with dispatcher = None
// FUNCTION: archive.source("...").posts(20).await
// OUTPUT: no panic, posts returned normally
```

**Scout is not touched.** Scout just sees `file.text` as `None` or `Some` via `ContentFetcher` — it has no knowledge of enrichment.

## Known Limitations (v1)

- **Instagram-only**: other platforms currently return empty file vecs — enrichment has no work to do
- **No budget cap**: monitor spend, add daily limits in v2 if needed
- **File content_hash is derived from post caption, not file bytes**: pre-existing issue, causes imperfect dedup. Fix separately.
- **No image description**: Claude vision prompt is OCR-focused (extract visible text only). Add description mode later if valuable for signal extraction.

## References

- Brainstorm: `docs/brainstorms/2026-02-23-media-enrichment-brainstorm.md`
- Existing `store.update_file_text()`: `modules/rootsignal-archive/src/store.rs:204-221`
- Existing workflow pattern: `modules/rootsignal-scout/src/workflows/scrape.rs`
- Existing `ContentFetcher` trait pattern: `modules/rootsignal-scout/src/pipeline/traits.rs:27-57`
- Claude vision plan: `docs/plans/2026-02-22-feat-instagram-stories-vision-pipeline-plan.md`
