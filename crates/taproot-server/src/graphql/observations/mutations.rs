use async_graphql::*;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use super::types::GqlObservation;

#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Approve,
    Reject,
}

#[derive(Default)]
pub struct ObservationMutation;

#[Object]
impl ObservationMutation {
    async fn review_observation(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        decision: ReviewDecision,
    ) -> Result<GqlObservation> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        // Verify observation exists and is pending
        let obs = taproot_domains::entities::Observation::find_by_id(id, pool)
            .await
            .map_err(|e| Error::new(format!("Failed to find observation: {e}")))?
            .ok_or_else(|| Error::new("Observation not found"))?;

        if obs.review_status != "pending" {
            return Err(Error::new(format!(
                "Observation already reviewed (status: {})",
                obs.review_status
            )));
        }

        let status = match decision {
            ReviewDecision::Approve => "approved",
            ReviewDecision::Reject => "rejected",
        };

        let updated = taproot_domains::entities::Observation::set_review_status(id, status, pool)
            .await
            .map_err(|e| Error::new(format!("Failed to update observation: {e}")))?;

        Ok(GqlObservation::from(updated))
    }
}
