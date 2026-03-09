//! Scheduling domain events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[seesaw_core::event(prefix = "scheduling")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchedulingEvent {
    /// A source or region should be re-scraped after a delay.
    ScrapeScheduled {
        scope: ScheduledScope,
        run_after: DateTime<Utc>,
        reason: String,
    },
}

/// What to re-scrape: specific sources or an entire region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "scope_type", rename_all = "snake_case")]
pub enum ScheduledScope {
    Sources { source_ids: Vec<Uuid> },
    Region { region: String },
}

