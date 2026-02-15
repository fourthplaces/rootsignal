use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::translation::restate::TranslateWorkflowClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRequest {
    pub snapshot_ids: Vec<String>,
    pub source_id: Option<String>,
}
impl_restate_serde!(ExtractRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResult {
    pub listing_ids: Vec<String>,
    pub extraction_count: u32,
    pub status: String,
}
impl_restate_serde!(ExtractResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[restate_sdk::workflow]
#[name = "ExtractWorkflow"]
pub trait ExtractWorkflow {
    async fn run(req: ExtractRequest) -> Result<ExtractResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ExtractWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl ExtractWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl ExtractWorkflow for ExtractWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: ExtractRequest,
    ) -> Result<ExtractResult, HandlerError> {
        ctx.set("status", "extracting".to_string());

        let source_id = req.source_id.as_ref().and_then(|s| s.parse::<Uuid>().ok());

        let mut all_listing_ids = Vec::new();
        let mut extraction_count: u32 = 0;

        for snapshot_id_str in &req.snapshot_ids {
            let snapshot_id: Uuid = snapshot_id_str
                .parse()
                .map_err(|e: uuid::Error| TerminalError::new(format!("Invalid UUID: {}", e)))?;

            // Step 1: AI extraction
            let deps = self.deps.clone();
            let extraction_ids_json: String = ctx
                .run(|| async move {
                    let ids =
                        crate::extraction::activities::extract_from_snapshot(snapshot_id, &deps)
                            .await
                            .map_err(|e| TerminalError::new(format!("Extraction failed: {}", e)))?;
                    serde_json::to_string(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>())
                        .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
                })
                .await?;

            let extraction_ids: Vec<String> = serde_json::from_str(&extraction_ids_json)
                .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

            extraction_count += extraction_ids.len() as u32;

            // Step 2: Normalize each extraction into entities/listings
            for extraction_id_str in &extraction_ids {
                let extraction_id: Uuid = extraction_id_str
                    .parse()
                    .map_err(|e: uuid::Error| TerminalError::new(format!("Invalid UUID: {}", e)))?;

                let deps = self.deps.clone();
                let sid = source_id;
                let listing_id_json: String = ctx
                    .run(|| async move {
                        let id = crate::extraction::activities::normalize_extraction(
                            extraction_id,
                            sid,
                            &deps,
                        )
                        .await
                        .map_err(|e| TerminalError::new(format!("Normalization failed: {}", e)))?;
                        serde_json::to_string(&id.map(|id| id.to_string()))
                            .map_err(|e| TerminalError::new(format!("Serialize: {}", e)).into())
                    })
                    .await?;

                let listing_id: Option<String> = serde_json::from_str(&listing_id_json)
                    .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

                if let Some(lid) = listing_id {
                    all_listing_ids.push(lid.clone());

                    // Fire TranslateWorkflow for this listing (non-blocking)
                    let deps = self.deps.clone();
                    let lid_for_translate = lid.clone();
                    let source_locale_json: String = ctx
                        .run(|| async move {
                            let listing_id: Uuid =
                                lid_for_translate.parse().map_err(|e: uuid::Error| {
                                    TerminalError::new(format!("Invalid UUID: {}", e))
                                })?;
                            let row = sqlx::query_as::<_, (String,)>(
                                "SELECT in_language FROM listings WHERE id = $1",
                            )
                            .bind(listing_id)
                            .fetch_one(deps.pool())
                            .await
                            .map_err(|e| TerminalError::new(format!("Fetch in_language: {}", e)))?;
                            Ok(row.0)
                        })
                        .await?;

                    let workflow_key = format!("listing-{}", lid);
                    let _ = ctx
                        .workflow_client::<TranslateWorkflowClient>(&workflow_key)
                        .run(crate::translation::TranslateRequest {
                            translatable_type: "listing".to_string(),
                            translatable_id: lid,
                            source_locale: source_locale_json,
                        })
                        .send();
                }
            }
        }

        ctx.set("status", "completed".to_string());

        Ok(ExtractResult {
            listing_ids: all_listing_ids,
            extraction_count,
            status: "completed".to_string(),
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
            .unwrap_or_else(|| "unknown".to_string()))
    }
}
