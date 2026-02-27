//! The dispatch loop.

use std::collections::VecDeque;
use std::marker::PhantomData;

use anyhow::Result;

use crate::traits::{EventLike, EventPersister, Reducer, Router};

/// Generic event dispatch engine.
///
/// Persist → reduce → route → recurse until settled.
/// Causal chaining is automatic: child events reference their trigger's seq.
pub struct Engine<E, S, D, Red, Rout, P>
where
    E: EventLike,
    S: Send,
    D: Send + Sync,
    Red: Reducer<E, S>,
    Rout: Router<E, S, D>,
    P: EventPersister,
{
    reducer: Red,
    router: Rout,
    persister: P,
    run_id: String,
    _phantom: PhantomData<fn() -> (E, S, D)>,
}

impl<E, S, D, Red, Rout, P> Engine<E, S, D, Red, Rout, P>
where
    E: EventLike,
    S: Send,
    D: Send + Sync,
    Red: Reducer<E, S>,
    Rout: Router<E, S, D>,
    P: EventPersister,
{
    pub fn new(reducer: Red, router: Rout, persister: P, run_id: String) -> Self {
        Self {
            reducer,
            router,
            persister,
            run_id,
            _phantom: PhantomData,
        }
    }

    /// Dispatch an event. Persists it, reduces state, routes to handler,
    /// and processes any emitted child events until the queue is empty.
    pub async fn dispatch(&self, event: E, state: &mut S, deps: &D) -> Result<()> {
        let mut queue: VecDeque<(E, Option<i64>)> = VecDeque::new();
        queue.push_back((event, None));

        while let Some((evt, parent_seq)) = queue.pop_front() {
            // 1. Persist with causal chain
            let stored = match parent_seq {
                None => {
                    self.persister
                        .persist(evt.event_type_str(), evt.to_persist_payload(), &self.run_id)
                        .await?
                }
                Some(parent) => {
                    self.persister
                        .persist_child(
                            parent,
                            evt.event_type_str(),
                            evt.to_persist_payload(),
                            &self.run_id,
                        )
                        .await?
                }
            };

            // 2. Reduce (pure state update)
            self.reducer.reduce(state, &evt);

            // 3. Route (may do I/O, may emit new events) — &S (auto-reborrows)
            let children = self.router.route(&evt, &stored, &*state, deps).await?;

            // 4. Enqueue children (chained off this event)
            for child in children {
                queue.push_back((child, Some(stored.seq)));
            }
        }

        Ok(())
    }

    /// Read-only access to the run ID.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}
