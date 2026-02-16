use async_graphql::*;
use uuid::Uuid;

use rootsignal_domains::signals::Signal;

use super::types::*;

#[derive(Default)]
pub struct SignalMutation;

#[Object]
impl SignalMutation {
    /// Flag a signal for correction (wrong type, wrong entity, expired, spam).
    async fn flag_signal(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        flag_type: FlagType,
        suggested_type: Option<SignalType>,
        comment: Option<String>,
    ) -> Result<bool> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        sqlx::query(
            r#"
            INSERT INTO signal_flags (signal_id, flag_type, suggested_type, comment)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(id)
        .bind(flag_type.as_str())
        .bind(suggested_type.map(|t| t.as_str()))
        .bind(comment.as_deref())
        .execute(pool)
        .await
        .map_err(|e| Error::new(format!("Failed to flag signal: {}", e)))?;

        Ok(true)
    }

    /// Delete all signals associated with a source.
    async fn delete_signals_by_source(&self, ctx: &Context<'_>, source_id: Uuid) -> Result<i32> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let count = Signal::delete_by_source(source_id, pool)
            .await
            .map_err(|e| Error::new(format!("Failed to delete signals: {}", e)))?;
        Ok(count as i32)
    }
}
