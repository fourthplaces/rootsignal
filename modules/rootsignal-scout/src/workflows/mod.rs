//! Restate durable workflows for the scout pipeline.
//!
//! Each pipeline phase is an independently invocable workflow. A thin orchestrator
//! (`FullScoutRunWorkflow`) composes them for a full scout run.
//!
//! Follows the same single-binary pattern as mntogether: each workflow impl holds
//! `Arc<ScoutDeps>` and constructs per-invocation resources from the shared deps.

pub mod bootstrap;
pub mod full_run;
pub mod news_scanner;
pub mod scrape;
pub mod situation_weaver;
pub mod supervisor;
pub mod synthesis;
pub mod types;

use std::sync::Arc;

use restate_sdk::prelude::*;
use rootsignal_archive::{Archive, ArchiveConfig, PageBackend, RestateDispatcher};
use rootsignal_graph::GraphClient;
use sqlx::PgPool;
use typed_builder::TypedBuilder;

/// Shared dependency container for all scout workflows.
///
/// Mirrors mntogether's `ServerDeps` pattern. Holds long-lived, cloneable
/// resources. Per-invocation resources (Archive, Embedder, Extractor) are
/// constructed from these deps at the start of each workflow invocation.
#[derive(Clone, TypedBuilder)]
pub struct ScoutDeps {
    pub graph_client: GraphClient,
    pub pg_pool: PgPool,
    pub anthropic_api_key: String,
    pub voyage_api_key: String,
    pub serper_api_key: String,
    #[builder(default)]
    pub apify_api_key: String,
    pub daily_budget_cents: u64,
    #[builder(default)]
    pub browserless_url: Option<String>,
    #[builder(default)]
    pub browserless_token: Option<String>,
    #[builder(default = 50)]
    pub max_web_queries_per_run: usize,
    #[builder(default)]
    pub restate_ingress_url: Option<String>,
}

impl ScoutDeps {
    /// Build the production SignalStore from these deps.
    pub fn build_store(&self, run_id: String) -> crate::pipeline::event_sourced_store::EventSourcedStore {
        crate::pipeline::build_signal_store(
            self.graph_client.clone(),
            self.pg_pool.clone(),
            run_id,
        )
    }

    /// Convenience constructor from Config — keeps API-side construction clean.
    pub fn from_config(
        graph_client: GraphClient,
        pg_pool: PgPool,
        config: &rootsignal_common::Config,
    ) -> Self {
        Self::builder()
            .graph_client(graph_client)
            .pg_pool(pg_pool)
            .anthropic_api_key(config.anthropic_api_key.clone())
            .voyage_api_key(config.voyage_api_key.clone())
            .serper_api_key(config.serper_api_key.clone())
            .apify_api_key(config.apify_api_key.clone())
            .daily_budget_cents(config.daily_budget_cents)
            .browserless_url(config.browserless_url.clone())
            .browserless_token(config.browserless_token.clone())
            .max_web_queries_per_run(config.max_web_queries_per_run)
            .restate_ingress_url(std::env::var("RESTATE_INGRESS_URL").ok().filter(|s| !s.is_empty()))
            .build()
    }
}

/// Create an `Archive` from the shared deps.
///
/// Each workflow invocation should call this to get a fresh archive instance.
pub fn create_archive(deps: &ScoutDeps) -> Arc<Archive> {
    let archive_config = ArchiveConfig {
        page_backend: match deps.browserless_url {
            Some(ref url) => PageBackend::Browserless {
                base_url: url.clone(),
                token: deps.browserless_token.clone(),
            },
            None => PageBackend::Chrome,
        },
        serper_api_key: deps.serper_api_key.clone(),
        apify_api_key: if deps.apify_api_key.is_empty() {
            None
        } else {
            Some(deps.apify_api_key.clone())
        },
    };

    let dispatcher = deps.restate_ingress_url.as_ref().map(|url| {
        Arc::new(RestateDispatcher::new(url.clone())) as Arc<dyn rootsignal_archive::WorkflowDispatcher>
    });

    Arc::new(Archive::new(deps.pg_pool.clone(), archive_config, dispatcher))
}

// ---------------------------------------------------------------------------
// Workflow helpers — shared across all workflows
// ---------------------------------------------------------------------------

/// Write phase status to the ScoutTask node.
/// Called by individual workflows to persist completion status for the admin UI.
pub async fn write_task_phase_status(deps: &ScoutDeps, task_id: &str, status: &str) {
    let writer = rootsignal_graph::GraphWriter::new(deps.graph_client.clone());
    if let Err(e) = writer.set_task_phase_status(task_id, status).await {
        tracing::warn!(%e, task_id, status, "Failed to write task phase status to graph");
    }
}

/// Read the `"status"` key from Restate workflow state. Returns `"pending"` if unset.
///
/// Every workflow exposes a `get_status` shared handler with identical logic;
/// this extracts the common body so each handler is a one-liner.
pub async fn read_workflow_status(ctx: &SharedWorkflowContext<'_>) -> Result<String, HandlerError> {
    Ok(ctx
        .get::<String>("status")
        .await?
        .unwrap_or_else(|| "pending".to_string()))
}


// ---------------------------------------------------------------------------
// Restate serde bridge macros (from mntogether)
// ---------------------------------------------------------------------------

/// Implement Restate SDK serialization traits for types that already have serde derives.
///
/// Bridges `serde::{Serialize, Deserialize}` to Restate's custom serialization traits
/// without needing the `Json<>` wrapper.
#[macro_export]
macro_rules! impl_restate_serde {
    ($type:ty) => {
        impl restate_sdk::serde::Serialize for $type {
            type Error = serde_json::Error;

            fn serialize(&self) -> Result<bytes::Bytes, Self::Error> {
                serde_json::to_vec(self).map(bytes::Bytes::from)
            }
        }

        impl restate_sdk::serde::Deserialize for $type {
            type Error = serde_json::Error;

            fn deserialize(bytes: &mut bytes::Bytes) -> Result<Self, Self::Error> {
                serde_json::from_slice(bytes)
            }
        }

        impl restate_sdk::serde::WithContentType for $type {
            fn content_type() -> &'static str {
                "application/json"
            }
        }
    };
}

