//! Event dispatch engine.
//!
//! Provides a generic event loop: persist → reduce → route → recurse until
//! settled. Events form causal chains via the underlying `EventStore`.
//!
//! Consumers define their domain by implementing `Reducer` (pure state updates)
//! and `Router` (side-effectful handlers that emit new events).

pub mod engine;
pub mod persist;
pub mod traits;

pub use engine::Engine;
pub use persist::MemoryEventSink;
pub use traits::{EventLike, EventPersister, Reducer, Router};
