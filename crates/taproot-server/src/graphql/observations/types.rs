use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

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
}

impl From<taproot_domains::entities::Observation> for GqlObservation {
    fn from(o: taproot_domains::entities::Observation) -> Self {
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

impl From<taproot_domains::entities::Investigation> for GqlInvestigation {
    fn from(i: taproot_domains::entities::Investigation) -> Self {
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
            taproot_domains::entities::Observation::find_by_investigation(self.id, pool)
                .await
                .unwrap_or_default();
        Ok(observations.into_iter().map(GqlObservation::from).collect())
    }
}
