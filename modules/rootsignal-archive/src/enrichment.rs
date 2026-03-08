// Media enrichment: trait boundary + routing logic.
//
// Archive detects unenriched media files after fetch and dispatches
// enrichment jobs via a WorkflowDispatcher trait. Production wires in
// a SpawnDispatcher (tokio::spawn); tests use MockDispatcher.

use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use rootsignal_common::types::ArchiveFile;

/// A single file to be enriched with extracted text.
#[derive(Debug, Clone)]
pub struct EnrichmentJob {
    pub file_id: Uuid,
    pub mime_type: String,
    pub media_bytes: Vec<u8>,
}

/// Trait boundary for dispatching enrichment work.
/// Archive calls this without knowing about the specific dispatch backend.
#[async_trait]
pub trait WorkflowDispatcher: Send + Sync {
    async fn enrich(&self, jobs: Vec<EnrichmentJob>) -> Result<()>;
}

/// Returns true if this mime type should be enriched.
fn is_enrichable_mime(mime: &str) -> bool {
    if mime == "image/svg+xml" {
        return false;
    }
    mime.starts_with("image/") || mime.starts_with("video/") || mime.starts_with("audio/")
}

/// Given a list of files, return those needing enrichment.
///
/// A file needs enrichment when:
/// - `text` is `None` (not yet processed — empty string means already attempted)
/// - `mime_type` matches image/* (except svg), video/*, or audio/*
pub fn files_needing_enrichment(files: &[ArchiveFile]) -> Vec<Uuid> {
    files
        .iter()
        .filter(|f| f.text.is_none() && is_enrichable_mime(&f.mime_type))
        .map(|f| f.id)
        .collect()
}

// ---------------------------------------------------------------------------
// SpawnDispatcher (production — runs enrichment directly via tokio::spawn)
// ---------------------------------------------------------------------------

const OCR_PROMPT: &str = "Extract all visible text from this image. Return only the text, nothing else. If no text is visible, return an empty string.";

/// Production dispatcher that processes enrichment jobs directly via `tokio::spawn`.
pub struct SpawnDispatcher {
    pg_pool: sqlx::PgPool,
    anthropic_api_key: String,
    openai_api_key: String,
}

impl SpawnDispatcher {
    pub fn new(pg_pool: sqlx::PgPool, anthropic_api_key: String, openai_api_key: String) -> Self {
        Self {
            pg_pool,
            anthropic_api_key,
            openai_api_key,
        }
    }
}

#[async_trait]
impl WorkflowDispatcher for SpawnDispatcher {
    async fn enrich(&self, jobs: Vec<EnrichmentJob>) -> Result<()> {
        let pg_pool = self.pg_pool.clone();
        let anthropic_key = self.anthropic_api_key.clone();
        let openai_key = self.openai_api_key.clone();
        let file_count = jobs.len();

        tracing::info!(file_count, "Dispatching enrichment via tokio::spawn");

        tokio::spawn(async move {
            const BATCH_SIZE: usize = 5;

            for batch in jobs.chunks(BATCH_SIZE) {
                let futures: Vec<_> = batch
                    .iter()
                    .map(|job| {
                        let pg_pool = pg_pool.clone();
                        let anthropic_key = anthropic_key.clone();
                        let openai_key = openai_key.clone();
                        let job = job.clone();
                        async move {
                            let file_id = job.file_id;
                            match enrich_single_file(&pg_pool, &anthropic_key, &openai_key, job)
                                .await
                            {
                                Ok(()) => {
                                    tracing::info!(%file_id, "enrichment: file complete");
                                }
                                Err(e) => {
                                    tracing::warn!(%file_id, error = %e, "enrichment: file failed, marking as attempted");
                                    let store = crate::store::Store::new(pg_pool.clone());
                                    let _ = store.update_file_text(file_id, "", None).await;
                                }
                            }
                        }
                    })
                    .collect();

                futures::future::join_all(futures).await;
            }
        });

        Ok(())
    }
}

/// Process a single file: route by mime type to Claude vision or Whisper.
async fn enrich_single_file(
    pg_pool: &sqlx::PgPool,
    anthropic_key: &str,
    openai_key: &str,
    job: EnrichmentJob,
) -> Result<()> {
    let text = if job.mime_type.starts_with("image/") {
        let claude = ai_client::Claude::new(anthropic_key, ai_client::models::SONNET_4);
        claude
            .describe_image(&job.media_bytes, &job.mime_type, OCR_PROMPT)
            .await?
    } else {
        let openai = ai_client::OpenAi::new(openai_key, "whisper-1");
        openai.transcribe(job.media_bytes, &job.mime_type).await?
    };

    let store = crate::store::Store::new(pg_pool.clone());
    store.update_file_text(job.file_id, &text, None).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// MockDispatcher (for tests)
// ---------------------------------------------------------------------------

/// Records `enrich()` calls for test assertions.
pub struct MockDispatcher {
    calls: Mutex<Vec<Vec<EnrichmentJob>>>,
}

impl MockDispatcher {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<Vec<EnrichmentJob>> {
        self.calls.lock().unwrap().clone()
    }

    pub fn total_files_dispatched(&self) -> usize {
        self.calls.lock().unwrap().iter().map(|c| c.len()).sum()
    }
}

#[async_trait]
impl WorkflowDispatcher for MockDispatcher {
    async fn enrich(&self, jobs: Vec<EnrichmentJob>) -> Result<()> {
        self.calls.lock().unwrap().push(jobs);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_file(mime_type: &str, text: Option<&str>) -> ArchiveFile {
        ArchiveFile {
            id: Uuid::new_v4(),
            url: "https://cdn.example.com/file".to_string(),
            content_hash: "abc123".to_string(),
            fetched_at: Utc::now(),
            title: None,
            mime_type: mime_type.to_string(),
            duration: None,
            page_count: None,
            text: text.map(|s| s.to_string()),
            text_language: None,
        }
    }

    #[test]
    fn image_file_with_null_text_needs_enrichment() {
        let file = test_file("image/jpeg", None);
        let ids = files_needing_enrichment(&[file.clone()]);
        assert_eq!(ids, vec![file.id]);
    }

    #[test]
    fn video_file_with_null_text_needs_enrichment() {
        let file = test_file("video/mp4", None);
        let ids = files_needing_enrichment(&[file.clone()]);
        assert_eq!(ids, vec![file.id]);
    }

    #[test]
    fn audio_file_with_null_text_needs_enrichment() {
        let file = test_file("audio/mpeg", None);
        let ids = files_needing_enrichment(&[file.clone()]);
        assert_eq!(ids, vec![file.id]);
    }

    #[test]
    fn already_enriched_file_is_skipped() {
        let file = test_file("image/jpeg", Some("hello"));
        let ids = files_needing_enrichment(&[file]);
        assert!(ids.is_empty());
    }

    #[test]
    fn empty_string_text_means_already_attempted() {
        let file = test_file("image/jpeg", Some(""));
        let ids = files_needing_enrichment(&[file]);
        assert!(ids.is_empty());
    }

    #[test]
    fn pdf_file_is_not_enriched() {
        let file = test_file("application/pdf", None);
        let ids = files_needing_enrichment(&[file]);
        assert!(ids.is_empty());
    }

    #[test]
    fn svg_file_is_not_enriched() {
        let file = test_file("image/svg+xml", None);
        let ids = files_needing_enrichment(&[file]);
        assert!(ids.is_empty());
    }

    #[test]
    fn mixed_files_returns_only_unenriched_media() {
        let jpeg = test_file("image/jpeg", None);
        let png = test_file("image/png", Some("hi"));
        let mp4 = test_file("video/mp4", None);
        let pdf = test_file("application/pdf", None);

        let ids = files_needing_enrichment(&[jpeg.clone(), png, mp4.clone(), pdf]);
        assert_eq!(ids, vec![jpeg.id, mp4.id]);
    }

    #[test]
    fn webp_and_gif_and_heic_need_enrichment() {
        let webp = test_file("image/webp", None);
        let gif = test_file("image/gif", None);
        let heic = test_file("image/heic", None);

        let ids = files_needing_enrichment(&[webp.clone(), gif.clone(), heic.clone()]);
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn empty_input_returns_empty() {
        let ids = files_needing_enrichment(&[]);
        assert!(ids.is_empty());
    }
}

