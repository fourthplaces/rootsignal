use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::warn;

use crate::tool::DynTool;
use crate::traits::{Agent, PromptBuilder};

/// Agent that tries a primary model, falling back to a secondary on failure.
#[derive(Clone)]
pub struct FallbackAgent {
    primary: Arc<dyn Agent>,
    fallback: Arc<dyn Agent>,
}

impl FallbackAgent {
    pub fn new(primary: impl Agent + 'static, fallback: impl Agent + 'static) -> Self {
        Self {
            primary: Arc::new(primary),
            fallback: Arc::new(fallback),
        }
    }
}

#[async_trait]
impl Agent for FallbackAgent {
    async fn extract_json(&self, system: &str, user: &str, schema: Value) -> anyhow::Result<Value> {
        match self.primary.extract_json(system, user, schema.clone()).await {
            Ok(v) => Ok(v),
            Err(e) => {
                warn!("Primary model failed, trying fallback: {e}");
                self.fallback.extract_json(system, user, schema).await
            }
        }
    }

    async fn chat(&self, system: &str, user: &str) -> anyhow::Result<String> {
        match self.primary.chat(system, user).await {
            Ok(v) => Ok(v),
            Err(e) => {
                warn!("Primary model failed, trying fallback: {e}");
                self.fallback.chat(system, user).await
            }
        }
    }

    fn with_tools(&self, tools: Vec<Arc<dyn DynTool>>) -> Box<dyn Agent> {
        Box::new(Self {
            primary: Arc::from(self.primary.with_tools(tools.clone())),
            fallback: Arc::from(self.fallback.with_tools(tools)),
        })
    }

    fn prompt(&self, input: &str) -> Box<dyn PromptBuilder> {
        self.primary.prompt(input)
    }
}
