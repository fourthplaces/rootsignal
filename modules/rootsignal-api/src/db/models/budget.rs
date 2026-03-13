use anyhow::Result;
use sqlx::PgPool;

pub struct BudgetConfig {
    pub daily_limit_cents: i64,
    pub per_run_max_cents: i64,
}

/// Load the single budget config row.
pub async fn load_config(pool: &PgPool) -> Result<BudgetConfig> {
    let row = sqlx::query_as::<_, (i64, i64)>(
        "SELECT daily_limit_cents, per_run_max_cents FROM budget_config LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    let (daily, per_run) = row.unwrap_or((0, 0));
    Ok(BudgetConfig {
        daily_limit_cents: daily,
        per_run_max_cents: per_run,
    })
}

/// Total spend across all runs today.
pub async fn daily_spend(pool: &PgPool) -> Result<i64> {
    let spent: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(spent_cents), 0) FROM runs \
         WHERE started_at >= CURRENT_DATE AT TIME ZONE 'UTC'",
    )
    .fetch_one(pool)
    .await?;
    Ok(spent)
}

/// Update budget config.
pub async fn set_config(
    pool: &PgPool,
    daily_limit_cents: i64,
    per_run_max_cents: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE budget_config SET daily_limit_cents = $1, per_run_max_cents = $2, updated_at = now()",
    )
    .bind(daily_limit_cents)
    .bind(per_run_max_cents)
    .execute(pool)
    .await?;
    Ok(())
}

/// Compute effective budget for a new run.
///
/// Returns `min(non-zero of: per_run_max, daily_remaining)`.
/// 0 means unlimited at either layer; if both are 0, returns `fallback_cents`.
pub async fn effective_budget(pool: &PgPool, fallback_cents: u64) -> u64 {
    let (config, spent) = match tokio::join!(load_config(pool), daily_spend(pool)) {
        (Ok(c), Ok(s)) => (c, s),
        _ => return fallback_cents,
    };

    let mut candidates: Vec<u64> = Vec::new();

    if config.per_run_max_cents > 0 {
        candidates.push(config.per_run_max_cents as u64);
    }

    if config.daily_limit_cents > 0 {
        let remaining = (config.daily_limit_cents - spent).max(0) as u64;
        candidates.push(remaining);
    }

    if candidates.is_empty() {
        fallback_cents
    } else {
        candidates.into_iter().min().unwrap_or(0)
    }
}
