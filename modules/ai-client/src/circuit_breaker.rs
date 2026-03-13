use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use std::sync::Mutex;
use serde_json::Value;
use tracing::warn;

use crate::tool::DynTool;
use crate::traits::{Agent, PromptBuilder};

/// Wraps an Agent and skips it for a cooldown period after failure.
///
/// On failure, records the timestamp. Subsequent calls within the cooldown
/// window return Err immediately — no network call. After cooldown expires,
/// tries again. Success closes the circuit; failure resets the timer.
#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<dyn Agent>,
    cooldown: Duration,
    tripped_at: Arc<Mutex<Option<Instant>>>,
}

impl CircuitBreaker {
    pub fn new(agent: impl Agent + 'static, cooldown: Duration) -> Self {
        Self {
            inner: Arc::new(agent),
            cooldown,
            tripped_at: Arc::new(Mutex::new(None)),
        }
    }

    fn is_open(&self) -> bool {
        let guard = self.tripped_at.lock().unwrap();
        match *guard {
            Some(at) => at.elapsed() < self.cooldown,
            None => false,
        }
    }

    fn trip(&self) {
        *self.tripped_at.lock().unwrap() = Some(Instant::now());
    }

    fn close(&self) {
        *self.tripped_at.lock().unwrap() = None;
    }
}

#[async_trait]
impl Agent for CircuitBreaker {
    async fn extract_json(&self, system: &str, user: &str, schema: Value) -> anyhow::Result<Value> {
        if self.is_open() {
            anyhow::bail!("circuit breaker open — skipping model");
        }
        match self.inner.extract_json(system, user, schema).await {
            Ok(v) => {
                self.close();
                Ok(v)
            }
            Err(e) => {
                warn!("Circuit breaker tripped: {e}");
                self.trip();
                Err(e)
            }
        }
    }

    async fn chat(&self, system: &str, user: &str) -> anyhow::Result<String> {
        if self.is_open() {
            anyhow::bail!("circuit breaker open — skipping model");
        }
        match self.inner.chat(system, user).await {
            Ok(v) => {
                self.close();
                Ok(v)
            }
            Err(e) => {
                warn!("Circuit breaker tripped: {e}");
                self.trip();
                Err(e)
            }
        }
    }

    fn with_tools(&self, tools: Vec<Arc<dyn DynTool>>) -> Box<dyn Agent> {
        Box::new(Self {
            inner: Arc::from(self.inner.with_tools(tools)),
            cooldown: self.cooldown,
            tripped_at: self.tripped_at.clone(),
        })
    }

    fn prompt(&self, input: &str) -> Box<dyn PromptBuilder> {
        self.inner.prompt(input)
    }
}
