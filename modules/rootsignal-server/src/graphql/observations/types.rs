use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::findings::types::GqlFinding;
use crate::graphql::investigations::types::GqlInvestigationStep;
use crate::graphql::signals::types::GqlSignal;

#[derive(SimpleObject, Clone)]
pub struct GqlObservation {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub observation_type: String,
    pub value: serde_json::Value,
    pub source: String,
    pub confidence: f32,
    pub investigation_id: Option<Uuid>,
    pub observed_at: DateTime<Utc>,
    pub review_status: String,
}

impl From<rootsignal_domains::investigations::Observation> for GqlObservation {
    fn from(o: rootsignal_domains::investigations::Observation) -> Self {
        Self {
            id: o.id,
            subject_type: o.subject_type,
            subject_id: o.subject_id,
            observation_type: o.observation_type,
            value: o.value,
            source: o.source,
            confidence: o.confidence,
            investigation_id: o.investigation_id,
            observed_at: o.observed_at,
            review_status: o.review_status,
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlInvestigation {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub trigger: String,
    pub status: String,
    pub summary_confidence: Option<f32>,
    pub summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::investigations::Investigation> for GqlInvestigation {
    fn from(i: rootsignal_domains::investigations::Investigation) -> Self {
        Self {
            id: i.id,
            subject_type: i.subject_type,
            subject_id: i.subject_id,
            trigger: i.trigger,
            status: i.status,
            summary_confidence: i.summary_confidence,
            summary: i.summary,
            started_at: i.started_at,
            completed_at: i.completed_at,
            created_at: i.created_at,
        }
    }
}

#[ComplexObject]
impl GqlInvestigation {
    async fn observations(&self, ctx: &Context<'_>) -> Result<Vec<GqlObservation>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let observations =
            rootsignal_domains::investigations::Observation::find_by_investigation(self.id, pool)
                .await
                .unwrap_or_default();
        Ok(observations.into_iter().map(GqlObservation::from).collect())
    }

    /// Tool call steps from the investigation (ordered by step_number).
    async fn steps(&self, ctx: &Context<'_>) -> Result<Vec<GqlInvestigationStep>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let steps =
            rootsignal_domains::findings::InvestigationStep::find_by_investigation(self.id, pool)
                .await
                .unwrap_or_default();
        Ok(steps.into_iter().map(GqlInvestigationStep::from).collect())
    }

    /// The finding produced by this investigation (if any).
    async fn finding(&self, ctx: &Context<'_>) -> Result<Option<GqlFinding>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let finding = sqlx::query_as::<_, rootsignal_domains::findings::Finding>(
            "SELECT * FROM findings WHERE investigation_id = $1 LIMIT 1",
        )
        .bind(self.id)
        .fetch_optional(pool)
        .await?;
        Ok(finding.map(GqlFinding::from))
    }

    /// The trigger signal that started this investigation.
    async fn signal(&self, ctx: &Context<'_>) -> Result<Option<GqlSignal>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        if self.subject_type != "signal" {
            return Ok(None);
        }
        let signal = rootsignal_domains::signals::Signal::find_by_id(self.subject_id, pool)
            .await
            .ok();
        Ok(signal.map(GqlSignal::from))
    }
}
