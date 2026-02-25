//! Generic, domain-agnostic append-only event store.
//!
//! Stores opaque JSONB facts with causal structure (parent_seq, caused_by_seq).
//! Zero knowledge of signals, Neo4j, or any domain concept.
//!
//! Consumers provide their own event types that serialize to `serde_json::Value`.

pub mod store;
pub mod types;

pub use store::{EventHandle, EventStore};
pub use types::{AppendEvent, StoredEvent};
