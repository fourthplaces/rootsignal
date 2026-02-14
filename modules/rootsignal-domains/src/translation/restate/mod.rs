use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use rootsignal_core::ServerDeps;
use uuid::Uuid;

use crate::translation::activities::translate::ALL_LOCALES;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateRequest {
    pub translatable_type: String,
    pub translatable_id: String,
    pub source_locale: String,
}
impl_restate_serde!(TranslateRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateResult {
    pub translation_count: u32,
    pub embedding_generated: bool,
    pub status: String,
}
impl_restate_serde!(TranslateResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[restate_sdk::workflow]
#[name = "TranslateWorkflow"]
pub trait TranslateWorkflow {
    async fn run(req: TranslateRequest) -> Result<TranslateResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct TranslateWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl TranslateWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl TranslateWorkflow for TranslateWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: TranslateRequest,
    ) -> Result<TranslateResult, HandlerError> {
        let record_id: Uuid = req.translatable_id.parse().map_err(|e: uuid::Error| {
            TerminalError::new(format!("Invalid UUID: {}", e))
        })?;

        let record_type = req.translatable_type.clone();
        let source_locale = req.source_locale.clone();
        let mut translation_count: u32 = 0;

        // Step 1: If source is not English, translate to English first (blocking)
        if source_locale != "en" {
            ctx.set("status", "translating_to_english".to_string());

            let deps = self.deps.clone();
            let rt = record_type.clone();
            let sl = source_locale.clone();
            let count_json: String = ctx
                .run(|| async move {
                    let ids = crate::translation::activities::translate::translate_record(
                        &rt, record_id, &sl, "en", &deps,
                    )
                    .await
                    .map_err(|e| {
                        TerminalError::new(format!("English translation failed: {}", e))
                    })?;
                    serde_json::to_string(&ids.len())
                        .map_err(|e| TerminalError::new(format!("Serialize: {}", e)).into())
                })
                .await?;

            let count: usize = serde_json::from_str(&count_json)
                .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;
            translation_count += count as u32;
        }

        // Step 2: Generate English embedding (blocking)
        ctx.set("status", "generating_embedding".to_string());

        let deps = self.deps.clone();
        let rt = record_type.clone();
        let sl = source_locale.clone();
        let embedding_json: String = ctx
            .run(|| async move {
                let id = crate::translation::activities::embed::generate_embedding(
                    &rt, record_id, &sl, &deps,
                )
                .await
                .map_err(|e| {
                    TerminalError::new(format!("Embedding generation failed: {}", e))
                })?;
                serde_json::to_string(&id.is_some())
                    .map_err(|e| TerminalError::new(format!("Serialize: {}", e)).into())
            })
            .await?;

        let embedding_generated: bool = serde_json::from_str(&embedding_json)
            .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        // Step 3: Translate to remaining locales (each as its own durable step)
        ctx.set("status", "translating_other_locales".to_string());

        let remaining_locales: Vec<&str> = ALL_LOCALES
            .iter()
            .copied()
            .filter(|l| *l != "en" && *l != source_locale.as_str())
            .collect();

        for target_locale in remaining_locales {
            let deps = self.deps.clone();
            let rt = record_type.clone();
            let sl = source_locale.clone();
            let tl = target_locale.to_string();
            let count_json: String = ctx
                .run(|| async move {
                    // Translate from source locale (or from English if source was non-English)
                    // Using English as intermediate for better quality
                    let from_locale = if sl == "en" { "en" } else { "en" };
                    let ids = crate::translation::activities::translate::translate_record(
                        &rt,
                        record_id,
                        from_locale,
                        &tl,
                        &deps,
                    )
                    .await
                    .map_err(|e| {
                        TerminalError::new(format!(
                            "Translation to {} failed: {}",
                            tl, e
                        ))
                    })?;
                    serde_json::to_string(&ids.len())
                        .map_err(|e| TerminalError::new(format!("Serialize: {}", e)).into())
                })
                .await?;

            let count: usize = serde_json::from_str(&count_json)
                .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;
            translation_count += count as u32;
        }

        ctx.set("status", "completed".to_string());

        Ok(TranslateResult {
            translation_count,
            embedding_generated,
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
