//! Core traits for the event engine.

use anyhow::Result;
use async_trait::async_trait;
use rootsignal_events::StoredEvent;

/// Events carry a type string and know how to serialize for the event store.
pub trait EventLike: Clone + Send + Sync + 'static {
    /// The event type string stored in the `event_type` column.
    fn event_type_str(&self) -> String;

    /// Serialize this event to the JSON payload stored in the event store.
    ///
    /// ScoutEvent overrides this to serialize World/System variants in the
    /// projector-compatible format (just the inner event, not the tagged wrapper).
    fn to_persist_payload(&self) -> serde_json::Value;
}

/// Pure state updates. No I/O, no side effects.
///
/// Called for every event before routing. Use for counters, accumulators,
/// and other state that can be derived from the event stream.
pub trait Reducer<E: EventLike, S: Send>: Send + Sync {
    fn reduce(&self, state: &mut S, event: &E);
}

/// Routes events to handlers. May perform I/O, emit new events.
///
/// Receives the persisted `StoredEvent` (for projection or other uses).
/// Returns zero or more child events that re-enter the dispatch loop.
#[async_trait]
pub trait Router<E: EventLike, S: Send, D: Send + Sync>: Send + Sync {
    async fn route(
        &self,
        event: &E,
        stored: &StoredEvent,
        state: &mut S,
        deps: &D,
    ) -> Result<Vec<E>>;
}

/// Persists events and returns a StoredEvent with sequence numbers.
///
/// Implemented by EventStore (postgres) and MemoryEventSink (tests).
/// Also implemented for `Arc<P>` so the sink can be shared for assertions.
#[async_trait]
pub trait EventPersister: Send + Sync {
    /// Persist a root event (no parent).
    async fn persist(
        &self,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent>;

    /// Persist a child event (causal chain from parent_seq).
    async fn persist_child(
        &self,
        parent_seq: i64,
        event_type: String,
        payload: serde_json::Value,
        run_id: &str,
    ) -> Result<StoredEvent>;
}
