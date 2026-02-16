use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_domains::findings::InvestigationStep;

#[derive(SimpleObject, Clone)]
pub struct GqlInvestigationStep {
    pub id: Uuid,
    pub investigation_id: Uuid,
    pub step_number: i32,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub page_snapshot_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<InvestigationStep> for GqlInvestigationStep {
    fn from(s: InvestigationStep) -> Self {
        Self {
            id: s.id,
            investigation_id: s.investigation_id,
            step_number: s.step_number,
            tool_name: s.tool_name,
            input: s.input,
            output: s.output,
            page_snapshot_id: s.page_snapshot_id,
            created_at: s.created_at,
        }
    }
}
