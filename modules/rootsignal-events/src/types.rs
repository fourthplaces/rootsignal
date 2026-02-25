//! Core types for the event store. Domain-agnostic.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An event as stored in Postgres. Returned by all read methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub event_type: String,
    pub parent_seq: Option<i64>,
    pub caused_by_seq: Option<i64>,
    pub run_id: Option<String>,
    pub actor: Option<String>,
    pub payload: serde_json::Value,
    pub schema_v: i16,
}

/// An event to be appended. The caller builds this; the store assigns seq/ts.
#[derive(Debug, Clone)]
pub struct AppendEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
    pub run_id: Option<String>,
    pub actor: Option<String>,
    pub schema_v: i16,
}

impl AppendEvent {
    /// Create an event from anything that serializes to JSON.
    /// The `event_type` is extracted from the serde tag.
    pub fn new(event_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            event_type: event_type.into(),
            payload,
            run_id: None,
            actor: None,
            schema_v: 1,
        }
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    pub fn with_schema_v(mut self, v: i16) -> Self {
        self.schema_v = v;
        self
    }
}
