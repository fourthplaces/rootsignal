//! Restate-backed seesaw runtime for durable handler execution.
//!
//! Each handler invocation is individually journaled through Restate's `ctx.run()`.
//! On replay, Restate returns the journaled output directly — the handler closure
//! is never polled, and the future is dropped.

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use restate_sdk::prelude::*;
use seesaw_core::handler::EventOutput;
use seesaw_core::runtime::Runtime;

use crate::impl_restate_serde;

/// Serializable journal entry for a single handler invocation's output.
#[derive(serde::Serialize, serde::Deserialize)]
struct JournaledHandlerOutput {
    events: Vec<JournaledEvent>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JournaledEvent {
    event_type: String,
    payload: serde_json::Value,
}

impl_restate_serde!(JournaledHandlerOutput);

/// Seesaw runtime that journals each handler invocation through Restate.
///
/// Borrows a `WorkflowContext` for the duration of settlement. Created per-settle:
///
/// ```ignore
/// let runtime = RestateRuntime::new(&ctx);
/// engine.emit(event).settled_with(&runtime).await?;
/// ```
pub struct RestateRuntime<'ctx> {
    ctx: &'ctx WorkflowContext<'ctx>,
}

impl<'ctx> RestateRuntime<'ctx> {
    pub fn new(ctx: &'ctx WorkflowContext<'ctx>) -> Self {
        Self { ctx }
    }
}

// SAFETY: WorkflowContext wraps &ContextInternal which contains Arc<Mutex<...>>.
// The reference is valid for the duration of settlement (borrowed via settled_with).
unsafe impl<'ctx> Send for RestateRuntime<'ctx> {}
unsafe impl<'ctx> Sync for RestateRuntime<'ctx> {}

impl<'ctx> Runtime for RestateRuntime<'ctx> {
    fn run(
        &self,
        handler_id: &str,
        execution: Pin<Box<dyn Future<Output = Result<Vec<EventOutput>>> + Send>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<EventOutput>>> + Send>> {
        let ctx = self.ctx;
        let name = handler_id.to_string();

        // The future captures &'ctx WorkflowContext but Runtime::run() requires
        // the return to be 'static (trait object default in Box). The future is
        // always awaited immediately within settle_inner() — it never escapes the
        // borrow scope. Transmute the lifetime to satisfy the trait signature.
        let fut: Pin<Box<dyn Future<Output = Result<Vec<EventOutput>>> + Send + 'ctx>> =
            Box::pin(async move {
                let journaled: JournaledHandlerOutput = ctx
                    .run(|| async move {
                        let outputs = execution
                            .await
                            .map_err(|e| TerminalError::new(e.to_string()))?;

                        Ok(JournaledHandlerOutput {
                            events: outputs
                                .into_iter()
                                .map(|o| JournaledEvent {
                                    event_type: o.event_type,
                                    payload: o.payload,
                                })
                                .collect(),
                        })
                    })
                    .name(name)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                Ok(journaled
                    .events
                    .into_iter()
                    .map(|j| EventOutput::from_serialized(j.event_type, j.payload))
                    .collect())
            });

        // SAFETY: The future borrows &'ctx WorkflowContext. settle_inner() calls
        // runtime.run() and immediately .await's the result — the future never
        // outlives the &self borrow. RestateRuntime is only used via settled_with()
        // which borrows the runtime for the duration of the settle loop.
        unsafe {
            std::mem::transmute::<
                Pin<Box<dyn Future<Output = Result<Vec<EventOutput>>> + Send + 'ctx>>,
                Pin<Box<dyn Future<Output = Result<Vec<EventOutput>>> + Send + 'static>>,
            >(fut)
        }
    }
}
