//! Adapter implementing `seesaw_core::EventStore` against rootsignal's Postgres event store.
//!
//! Stateless — `run_id` and `schema_v` arrive via `NewEvent.metadata`, set by
//! `engine.with_event_metadata()` at construction time.

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use uuid::Uuid;

use rootsignal_events::{AppendEvent, EventStore, StoredEvent};
use seesaw_core::event_store::{NewEvent, PersistedEvent};

/// Adapter bridging seesaw's `EventStore` trait to rootsignal's Postgres-backed `EventStore`.
pub struct SeesawEventStoreAdapter {
    inner: EventStore,
}

impl SeesawEventStoreAdapter {
    pub fn new(inner: EventStore) -> Self {
        Self { inner }
    }
}

impl seesaw_core::event_store::EventStore for SeesawEventStoreAdapter {
    fn append(
        &self,
        event: NewEvent,
    ) -> Pin<Box<dyn Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let run_id = event
                .metadata
                .get("run_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let schema_v = event
                .metadata
                .get("schema_v")
                .and_then(|v| v.as_i64())
                .unwrap_or(1) as i16;

            let handler_id = event
                .metadata
                .get("handler_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let mut append = AppendEvent::new(event.event_type, event.payload)
                .with_id(event.event_id)
                .with_schema_v(schema_v);

            if let Some(run_id) = run_id {
                append = append.with_run_id(run_id);
            }
            if let Some(handler_id) = handler_id {
                append = append.with_handler_id(handler_id);
            }
            if let Some(parent_id) = event.parent_id {
                append = append.with_parent_id(parent_id);
            }

            append.correlation_id = Some(event.correlation_id);
            append.aggregate_type = event.aggregate_type;
            append.aggregate_id = event.aggregate_id;

            let handle = self.inner.append(append).await?;
            Ok(handle.seq() as u64)
        })
    }

    fn load_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>>> + Send + '_>> {
        let agg_type = aggregate_type.to_string();
        Box::pin(async move {
            let events = self.inner.load_aggregate_stream(&agg_type, aggregate_id).await?;
            Ok(events.into_iter().map(stored_to_persisted).collect())
        })
    }

    fn load_stream_from(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
        after_position: u64,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>>> + Send + '_>> {
        let agg_type = aggregate_type.to_string();
        Box::pin(async move {
            let events = self
                .inner
                .load_aggregate_stream_from(&agg_type, aggregate_id, after_position as i64)
                .await?;
            Ok(events.into_iter().map(stored_to_persisted).collect())
        })
    }

    fn load_global_from(
        &self,
        after_position: u64,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>>> + Send + '_>> {
        Box::pin(async move {
            let events = self.inner.read_from(after_position as i64, limit).await?;
            Ok(events.into_iter().map(stored_to_persisted).collect())
        })
    }
}

fn stored_to_persisted(e: StoredEvent) -> PersistedEvent {
    PersistedEvent {
        position: e.seq as u64,
        event_id: e.id.unwrap_or_else(Uuid::new_v4),
        parent_id: e.parent_id,
        correlation_id: e.correlation_id.unwrap_or_else(Uuid::new_v4),
        event_type: e.event_type,
        payload: e.payload,
        created_at: e.ts,
        aggregate_type: e.aggregate_type,
        aggregate_id: e.aggregate_id,
        version: None, // Postgres seq serves as the global ordering
        metadata: {
            let mut map = serde_json::Map::new();
            if let Some(run_id) = e.run_id {
                map.insert("run_id".to_string(), serde_json::Value::String(run_id));
            }
            map.insert("schema_v".to_string(), serde_json::json!(e.schema_v));
            if let Some(actor) = e.actor {
                map.insert("actor".to_string(), serde_json::Value::String(actor));
            }
            if let Some(handler_id) = e.handler_id {
                map.insert("handler_id".to_string(), serde_json::Value::String(handler_id));
            }
            map
        },
    }
}
