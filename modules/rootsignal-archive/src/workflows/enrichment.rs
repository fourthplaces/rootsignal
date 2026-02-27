//! Restate durable workflow for media enrichment.
//!
//! Processes media files: images go through Claude vision (OCR),
//! video/audio goes through OpenAI Whisper (transcription).
//! Each file is processed in its own `ctx.run()` block for crash safety.

use std::sync::Arc;

use base64::Engine;
use restate_sdk::prelude::*;
use tracing::{info, warn};

use super::types::{EmptyRequest, EnrichmentFileRequest, EnrichmentRequest, EnrichmentResult};
use super::ArchiveDeps;

const OCR_PROMPT: &str = "Extract all visible text from this image. Return only the text, nothing else. If no text is visible, return an empty string.";

#[restate_sdk::workflow]
#[name = "EnrichmentWorkflow"]
pub trait EnrichmentWorkflow {
    async fn run(req: EnrichmentRequest) -> Result<EnrichmentResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct EnrichmentWorkflowImpl {
    deps: Arc<ArchiveDeps>,
}

impl EnrichmentWorkflowImpl {
    pub fn with_deps(deps: Arc<ArchiveDeps>) -> Self {
        Self { deps }
    }
}

impl EnrichmentWorkflow for EnrichmentWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: EnrichmentRequest,
    ) -> Result<EnrichmentResult, HandlerError> {
        let total = req.files.len();
        ctx.set("status", format!("Enriching {total} files..."));

        let mut enriched = 0u32;
        let mut failed = 0u32;

        for file_req in req.files {
            let deps = self.deps.clone();
            let file_id = file_req.file_id;

            match ctx.run(|| enrich_single_file(deps, file_req)).await {
                Ok(()) => {
                    enriched += 1;
                    info!(%file_id, "enrichment: file complete");
                }
                Err(e) => {
                    failed += 1;
                    warn!(%file_id, error = %e, "enrichment: file failed, marking as attempted");
                    // Mark as attempted so we don't re-dispatch
                    let deps = self.deps.clone();
                    let fid = file_id;
                    let _ = ctx
                        .run(|| async move {
                            let store = crate::store::Store::new(deps.pg_pool.clone());
                            store.update_file_text(fid, "", None).await.map_err(
                                |e| -> HandlerError { TerminalError::new(e.to_string()).into() },
                            )?;
                            Ok(())
                        })
                        .await;
                }
            }
        }

        ctx.set(
            "status",
            format!("Complete: {enriched} enriched, {failed} failed"),
        );
        info!(enriched, failed, "EnrichmentWorkflow complete");

        Ok(EnrichmentResult {
            files_enriched: enriched,
            files_failed: failed,
        })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        Ok(ctx
            .get::<String>("status")
            .await?
            .unwrap_or_else(|| "pending".to_string()))
    }
}

/// Process a single file: route by mime type to Claude vision or Whisper.
async fn enrich_single_file(
    deps: Arc<ArchiveDeps>,
    file_req: EnrichmentFileRequest,
) -> Result<(), HandlerError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&file_req.media_bytes_b64)
        .map_err(|e| -> HandlerError {
            TerminalError::new(format!("base64 decode failed: {e}")).into()
        })?;

    let text = if file_req.mime_type.starts_with("image/") {
        let claude = ai_client::Claude::new(&deps.anthropic_api_key, "claude-sonnet-4-20250514");
        claude
            .describe_image(&bytes, &file_req.mime_type, OCR_PROMPT)
            .await
            .map_err(|e| -> HandlerError {
                TerminalError::new(format!("Claude vision failed: {e}")).into()
            })?
    } else {
        let openai = ai_client::OpenAi::new(&deps.openai_api_key, "whisper-1");
        openai
            .transcribe(bytes, &file_req.mime_type)
            .await
            .map_err(|e| -> HandlerError {
                TerminalError::new(format!("Whisper failed: {e}")).into()
            })?
    };

    let store = crate::store::Store::new(deps.pg_pool.clone());
    store
        .update_file_text(file_req.file_id, &text, None)
        .await
        .map_err(|e| -> HandlerError {
            TerminalError::new(format!("DB update failed: {e}")).into()
        })?;

    Ok(())
}
