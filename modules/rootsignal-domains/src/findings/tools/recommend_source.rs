use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::findings::InvestigationStep;

#[derive(Debug, Deserialize)]
pub struct RecommendSourceArgs {
    pub url: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct RecommendSourceOutput {
    pub recorded: bool,
    pub url: String,
}

pub struct RecommendSourceTool {
    pool: PgPool,
    investigation_id: Uuid,
}

impl RecommendSourceTool {
    pub fn new(pool: PgPool, investigation_id: Uuid) -> Self {
        Self {
            pool,
            investigation_id,
        }
    }
}

#[derive(Debug)]
pub struct RecommendSourceError(anyhow::Error);

impl std::fmt::Display for RecommendSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RecommendSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[async_trait]
impl Tool for RecommendSourceTool {
    const NAME: &'static str = "recommend_source";
    type Error = RecommendSourceError;
    type Args = RecommendSourceArgs;
    type Output = RecommendSourceOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Suggest a new URL/source that should be monitored for ongoing coverage of the phenomenon you're investigating. The recommendation will be reviewed by a human.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL of the source to recommend monitoring"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Why this source should be monitored"
                    }
                },
                "required": ["url", "reason"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let step_number = InvestigationStep::next_step_number(self.investigation_id, &self.pool)
            .await
            .map_err(|e| RecommendSourceError(e))?;

        // Log the recommendation as an investigation step — processed after investigation completes
        InvestigationStep::create(
            self.investigation_id,
            step_number,
            Self::NAME,
            serde_json::json!({
                "url": args.url,
                "reason": args.reason,
            }),
            serde_json::json!({ "recorded": true }),
            None,
            &self.pool,
        )
        .await
        .map_err(|e| RecommendSourceError(e))?;

        Ok(RecommendSourceOutput {
            recorded: true,
            url: args.url,
        })
    }
}
