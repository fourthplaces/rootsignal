//! Scheduling domain events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[causal::event(prefix = "scheduling")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchedulingEvent {
    /// A source or region should be re-scraped after a delay.
    ScrapeScheduled {
        scope: ScheduledScope,
        run_after: DateTime<Utc>,
        reason: String,
    },

    /// A recurring schedule was created.
    ScheduleCreated {
        schedule_id: String,
        flow_type: String,
        scope: serde_json::Value,
        timeout: u64,
        #[serde(default)]
        base_timeout: Option<u64>,
        #[serde(default = "default_recurring")]
        recurring: bool,
        region_id: Option<String>,
    },

    /// A schedule was enabled or disabled.
    ScheduleToggled {
        schedule_id: String,
        enabled: bool,
    },

    /// A schedule fired and spawned a run.
    ScheduleTriggered {
        schedule_id: String,
        run_id: String,
    },

    /// A schedule was soft-deleted.
    ScheduleDeleted {
        schedule_id: String,
    },

    /// Adjust a schedule's timeout (exponential backoff or reset).
    ScheduleCadenceAdjusted {
        schedule_id: String,
        new_timeout: i32,
        reason: String,
    },
}

fn default_recurring() -> bool { true }

/// What to re-scrape: specific sources or an entire region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "scope_type", rename_all = "snake_case")]
pub enum ScheduledScope {
    Sources { source_ids: Vec<Uuid> },
    Region { region: String },
}

