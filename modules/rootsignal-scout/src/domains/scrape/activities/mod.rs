//! Scrape domain activity functions: pure logic extracted from handlers.
//!
//! Each function takes specific inputs and returns accumulated output.
//! No `&mut PipelineState` — state flows through `ScrapeOutput`.

pub(crate) mod scrape_phase;

use crate::infra::run_log::RunLogger;

pub async fn build_run_logger(
    run_id: &str,
    region_name: &str,
    pg_pool: Option<&sqlx::PgPool>,
) -> RunLogger {
    match pg_pool {
        Some(pool) => {
            RunLogger::new(run_id.to_string(), region_name.to_string(), pool.clone()).await
        }
        None => RunLogger::noop(),
    }
}
