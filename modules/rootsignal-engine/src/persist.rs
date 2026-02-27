//! EventPersister implementations.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use rootsignal_events::{AppendEvent, EventStore, StoredEvent};

use crate::traits::EventPersister;

// ---------------------------------------------------------------------------
// EventStore adapter (production — postgres)
// ---------------------------------------------------------------------------

#[async_trait]
impl EventPersister for EventStore {
    async fn persist(
        &self,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent> {
        let append = AppendEvent::new(event_type, payload).with_run_id(run_id);
        self.append_and_read(append).await
    }

    async fn persist_child(
        &self,
        parent_seq: i64,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent> {
        let append = AppendEvent::new(event_type, payload).with_run_id(run_id);
        self.append_child_and_read(parent_seq, append).await
    }
}

// ---------------------------------------------------------------------------
// MemoryEventSink (tests — no database required)
// ---------------------------------------------------------------------------

/// In-memory event sink for testing. Generates fake StoredEvents with
/// incrementing sequence numbers. Thread-safe.
pub struct MemoryEventSink {
    next_seq: AtomicI64,
    events: Mutex<Vec<StoredEvent>>,
}

impl MemoryEventSink {
    pub fn new() -> Self {
        Self {
            next_seq: AtomicI64::new(1),
            events: Mutex::new(Vec::new()),
        }
    }

    /// Read all persisted events (for test assertions).
    pub fn events(&self) -> Vec<StoredEvent> {
        self.events.lock().unwrap().clone()
    }

    fn make_stored(
        &self,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
        caused_by_seq: Option<i64>,
    ) -> StoredEvent {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let stored = StoredEvent {
            seq,
            ts: Utc::now(),
            event_type,
            parent_seq: caused_by_seq,
            caused_by_seq,
            run_id: Some(run_id.to_string()),
            actor: None,
            payload,
            schema_v: 1,
        };
        self.events.lock().unwrap().push(stored.clone());
        stored
    }
}

#[async_trait]
impl EventPersister for MemoryEventSink {
    async fn persist(
        &self,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent> {
        Ok(self.make_stored(event_type, payload, run_id, None))
    }

    async fn persist_child(
        &self,
        parent_seq: i64,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent> {
        Ok(self.make_stored(event_type, payload, run_id, Some(parent_seq)))
    }
}

// ---------------------------------------------------------------------------
// Arc<P> blanket — lets tests share the sink for assertions
// ---------------------------------------------------------------------------

#[async_trait]
impl<P: EventPersister + ?Sized> EventPersister for Arc<P> {
    async fn persist(
        &self,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent> {
        (**self).persist(event_type, payload, run_id).await
    }

    async fn persist_child(
        &self,
        parent_seq: i64,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent> {
        (**self)
            .persist_child(parent_seq, event_type, payload, run_id)
            .await
    }
}
