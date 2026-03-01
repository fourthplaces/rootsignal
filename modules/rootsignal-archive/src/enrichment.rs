// Media enrichment: trait boundary + routing logic.
//
// Archive detects unenriched media files after fetch and dispatches
// enrichment jobs via a WorkflowDispatcher trait. Production wires in
// a Restate-backed dispatcher; tests use MockDispatcher.

use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use rootsignal_common::types::ArchiveFile;
use base64::Engine;

/// A single file to be enriched with extracted text.
#[derive(Debug, Clone)]
pub struct EnrichmentJob {
    pub file_id: Uuid,
    pub mime_type: String,
    pub media_bytes: Vec<u8>,
}

/// Trait boundary for dispatching enrichment work.
/// Archive calls this without knowing about Restate or any specific backend.
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
// RestateDispatcher (production)
// ---------------------------------------------------------------------------

/// Production dispatcher that POSTs enrichment jobs to the Restate ingress.
/// Uses `/send` suffix for fire-and-forget semantics.
pub struct RestateDispatcher {
    http: reqwest::Client,
    ingress_url: String,
}

impl RestateDispatcher {
    pub fn new(ingress_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            ingress_url: ingress_url.into(),
        }
    }
}

#[async_trait]
impl WorkflowDispatcher for RestateDispatcher {
    async fn enrich(&self, jobs: Vec<EnrichmentJob>) -> Result<()> {

        // Derive workflow key from sorted file IDs (idempotent)
        let mut ids: Vec<String> = jobs.iter().map(|j| j.file_id.to_string()).collect();
        ids.sort();
        let key = rootsignal_common::content_hash(&ids.join(",")).to_string();

        let files: Vec<crate::workflows::types::EnrichmentFileRequest> = jobs
            .into_iter()
            .map(|j| crate::workflows::types::EnrichmentFileRequest {
                file_id: j.file_id,
                mime_type: j.mime_type,
                media_bytes_b64: base64::engine::general_purpose::STANDARD.encode(&j.media_bytes),
            })
            .collect();

        let body = serde_json::json!({ "files": files });
        let url = format!("{}/EnrichmentWorkflow/{key}/run/send", self.ingress_url);

        tracing::info!(
            url = url.as_str(),
            file_count = files.len(),
            "Dispatching enrichment via Restate"
        );

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Restate enrichment dispatch failed ({}): {}",
                status,
                error_text
            );
        }

        Ok(())
    }
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

