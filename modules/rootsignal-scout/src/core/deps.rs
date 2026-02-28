//! Immutable dependencies for the engine dispatch loop.

use std::sync::Arc;

use rootsignal_common::ScoutScope;

use rootsignal_graph::GraphClient;

use crate::infra::embedder::TextEmbedder;
use crate::traits::{ContentFetcher, SignalReader};

/// Immutable dependencies passed to `Engine::dispatch()`.
///
/// Does NOT include the GraphProjector — that lives on the ScoutRouter,
/// since only the router (not individual handlers) needs to project
/// World/System events to Neo4j.
#[derive(Clone)]
pub struct PipelineDeps {
    pub store: Arc<dyn SignalReader>,
    pub embedder: Arc<dyn TextEmbedder>,
    pub region: Option<ScoutScope>,
    pub run_id: String,
    pub fetcher: Option<Arc<dyn ContentFetcher>>,
    pub anthropic_api_key: Option<String>,
    pub graph_client: Option<GraphClient>,
}
