use std::sync::Arc;

use anyhow::Result;

use ai_client::Agent;
use rootsignal_graph::GraphQueries;
use crate::infra::embedder::TextEmbedder;

use super::types::CoalescingResult;

pub struct Coalescer {
    graph: Arc<dyn GraphQueries>,
    ai: Arc<dyn Agent>,
    embedder: Arc<dyn TextEmbedder>,
}

impl Coalescer {
    pub fn new(
        graph: Arc<dyn GraphQueries>,
        ai: Arc<dyn Agent>,
        embedder: Arc<dyn TextEmbedder>,
    ) -> Self {
        Self { graph, ai, embedder }
    }

    /// Run seed mode (new groups from ungrouped signals) + feed mode (grow existing groups).
    pub async fn run(&self) -> Result<CoalescingResult> {
        // Phase 3 will implement the 3-round coalescing workflow here.
        // For now, return empty results.
        Ok(CoalescingResult {
            new_groups: vec![],
            fed_signals: vec![],
            refined_queries: vec![],
        })
    }
}
