use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::warn;

use crate::tool::DynTool;
use crate::traits::{Agent, PromptBuilder};

/// Agent that tries models in order, falling back to the next on failure.
#[derive(Clone)]
pub struct FallbackAgent {
    agents: Vec<Arc<dyn Agent>>,
}

impl FallbackAgent {
    pub fn new(agents: Vec<Arc<dyn Agent>>) -> Self {
        assert!(!agents.is_empty(), "FallbackAgent requires at least one agent");
        Self { agents }
    }
}

#[async_trait]
impl Agent for FallbackAgent {
    async fn extract_json(&self, system: &str, user: &str, schema: Value) -> anyhow::Result<Value> {
        let last = self.agents.len() - 1;
        for (i, agent) in self.agents.iter().enumerate() {
            match agent.extract_json(system, user, schema.clone()).await {
                Ok(v) => return Ok(v),
                Err(e) if i < last => {
                    warn!("Model {}/{} failed, trying next: {e}", i + 1, self.agents.len());
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    async fn chat(&self, system: &str, user: &str) -> anyhow::Result<String> {
        let last = self.agents.len() - 1;
        for (i, agent) in self.agents.iter().enumerate() {
            match agent.chat(system, user).await {
                Ok(v) => return Ok(v),
                Err(e) if i < last => {
                    warn!("Model {}/{} failed, trying next: {e}", i + 1, self.agents.len());
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    fn with_tools(&self, tools: Vec<Arc<dyn DynTool>>) -> Box<dyn Agent> {
        Box::new(Self {
            agents: self.agents.iter()
                .map(|a| Arc::from(a.with_tools(tools.clone())))
                .collect(),
        })
    }

    fn prompt(&self, input: &str) -> Box<dyn PromptBuilder> {
        self.agents[0].prompt(input)
    }
}
