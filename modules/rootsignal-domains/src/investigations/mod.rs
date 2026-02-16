pub mod investigation;
pub mod observation;
pub mod proposal;
pub mod restate;
pub mod tools;

pub use investigation::Investigation;
pub use observation::Observation;
pub use restate::InvestigateWorkflowImpl;

use anyhow::Result;
use rootsignal_core::ServerDeps;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use ai_client::traits::{Agent, PromptBuilder};

use crate::entities::Entity;
use tools::{InternalSignalHistoryTool, TavilyEntitySearchTool, WhoisLookupTool};

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct InvestigationSummary {
    /// Confidence score from 0.0 to 1.0
    confidence: f32,
    /// Summary of the investigation findings
    summary: String,
}

pub async fn run_investigation(
    subject_type: &str,
    subject_id: Uuid,
    trigger: &str,
    deps: &Arc<ServerDeps>,
) -> Result<Investigation> {
    let pool = deps.pool();

    // Create investigation record
    let investigation = Investigation::create(subject_type, subject_id, trigger, pool).await?;
    Investigation::update_status(investigation.id, "running", pool).await?;

    info!(
        investigation_id = %investigation.id,
        subject_type,
        subject_id = %subject_id,
        "Starting investigation"
    );

    // Load the entity for context
    let entity = Entity::find_by_id(subject_id, pool).await?;

    // Build the agent with investigation tools
    let agent = (*deps.ai)
        .clone()
        .tool(WhoisLookupTool::new(deps.http_client.clone()))
        .tool(TavilyEntitySearchTool::new(deps.web_searcher.clone()))
        .tool(InternalSignalHistoryTool::new(deps.db_pool.clone()));

    // Build the investigation prompt
    let mut prompt_parts = vec![format!(
        "Investigate this entity to assess its legitimacy:\n\nName: {}\nType: {}",
        entity.name, entity.entity_type
    )];

    if let Some(ref website) = entity.website {
        prompt_parts.push(format!("Website: {}", website));
    }
    if let Some(ref description) = entity.description {
        prompt_parts.push(format!("Description: {}", description));
    }

    prompt_parts.push(
        "\nUse the available tools to gather evidence. Look up the domain registration, search for the entity online, and check our internal signal history. Then provide a final assessment.".to_string(),
    );

    let prompt_text = prompt_parts.join("\n");

    // Run the multi-turn agent
    let preamble = deps.prompts.investigation_prompt();

    let response = agent
        .prompt(&prompt_text)
        .preamble(preamble)
        .multi_turn(10)
        .send()
        .await;

    match response {
        Ok(text) => {
            // Extract structured summary from agent response
            let model = &deps.file_config.models.investigation;
            let extraction_prompt = "Extract the investigation confidence score and summary from the following agent transcript.";
            let parsed = deps.ai.extract::<InvestigationSummary>(model, extraction_prompt, &text).await;
            let (confidence, summary) = match parsed {
                Ok(s) => (s.confidence.clamp(0.0, 1.0), s.summary),
                Err(_) => (0.5, text.clone()),
            };

            // Store the final assessment as an observation
            Observation::create(
                subject_type,
                subject_id,
                "agent_assessment",
                serde_json::json!({
                    "summary": &summary,
                    "raw_response": &text,
                }),
                "investigation_agent",
                confidence,
                Some(investigation.id),
                pool,
            )
            .await?;

            Investigation::complete(investigation.id, &summary, confidence, pool).await?;

            info!(
                investigation_id = %investigation.id,
                confidence,
                "Investigation completed"
            );

            Investigation::find_by_id(investigation.id, pool).await
        }
        Err(e) => {
            let error_msg = format!("Investigation failed: {}", e);
            Investigation::update_status(investigation.id, "failed", pool).await?;

            Observation::create(
                subject_type,
                subject_id,
                "agent_error",
                serde_json::json!({ "error": &error_msg }),
                "investigation_agent",
                0.0,
                Some(investigation.id),
                pool,
            )
            .await?;

            Err(e)
        }
    }
}

