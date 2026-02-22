//! Restate durable workflows for the scout pipeline.
//!
//! Each pipeline phase is an independently invocable workflow. A thin orchestrator
//! (`FullScoutRunWorkflow`) composes them for a full scout run.
//!
//! Follows the same single-binary pattern as mntogether: each workflow impl holds
//! `Arc<ScoutDeps>` and constructs per-invocation resources from the shared deps.

pub mod actor_discovery;
pub mod bootstrap;
pub mod full_run;
pub mod scrape;
pub mod situation_weaver;
pub mod supervisor;
pub mod synthesis;
pub mod types;

use std::sync::Arc;

use rootsignal_archive::{Archive, ArchiveConfig, PageBackend};
use rootsignal_graph::GraphClient;
use sqlx::PgPool;

/// Shared dependency container for all scout workflows.
///
/// Mirrors mntogether's `ServerDeps` pattern. Holds long-lived, cloneable
/// resources. Per-invocation resources (Archive, Embedder, Extractor) are
/// constructed from these deps at the start of each workflow invocation.
#[derive(Clone)]
pub struct ScoutDeps {
    pub graph_client: GraphClient,
    pub pg_pool: PgPool,
    pub anthropic_api_key: String,
    pub voyage_api_key: String,
    pub serper_api_key: String,
    pub apify_api_key: String,
    pub daily_budget_cents: u64,
}

impl ScoutDeps {
    pub fn new(
        graph_client: GraphClient,
        pg_pool: PgPool,
        config: &rootsignal_common::Config,
    ) -> Self {
        Self {
            graph_client,
            pg_pool,
            anthropic_api_key: config.anthropic_api_key.clone(),
            voyage_api_key: config.voyage_api_key.clone(),
            serper_api_key: config.serper_api_key.clone(),
            apify_api_key: config.apify_api_key.clone(),
            daily_budget_cents: config.daily_budget_cents,
        }
    }
}

/// Create an `Archive` (FetchBackend) from the shared deps.
///
/// Each workflow invocation should call this to get a fresh archive instance.
pub fn create_archive(deps: &ScoutDeps, run_label: &str) -> Arc<dyn rootsignal_archive::FetchBackend> {
    let archive_config = ArchiveConfig {
        page_backend: match std::env::var("BROWSERLESS_URL") {
            Ok(url) => {
                let token = std::env::var("BROWSERLESS_TOKEN").ok();
                PageBackend::Browserless { base_url: url, token }
            }
            Err(_) => PageBackend::Chrome,
        },
        serper_api_key: deps.serper_api_key.clone(),
        apify_api_key: if deps.apify_api_key.is_empty() {
            None
        } else {
            Some(deps.apify_api_key.clone())
        },
        anthropic_api_key: if deps.anthropic_api_key.is_empty() {
            None
        } else {
            Some(deps.anthropic_api_key.clone())
        },
    };

    Arc::new(Archive::new(
        deps.pg_pool.clone(),
        archive_config,
        uuid::Uuid::new_v4(),
        run_label.to_string(),
    ))
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

/// Implement Restate SDK serialization traits for `Vec<T>`.
#[macro_export]
macro_rules! impl_restate_serde_vec {
    ($type:ty) => {
        impl restate_sdk::serde::Serialize for Vec<$type> {
            type Error = serde_json::Error;

            fn serialize(&self) -> Result<bytes::Bytes, Self::Error> {
                serde_json::to_vec(self).map(bytes::Bytes::from)
            }
        }

        impl restate_sdk::serde::Deserialize for Vec<$type> {
            type Error = serde_json::Error;

            fn deserialize(bytes: &mut bytes::Bytes) -> Result<Self, Self::Error> {
                serde_json::from_slice(bytes)
            }
        }

        impl restate_sdk::serde::WithContentType for Vec<$type> {
            fn content_type() -> &'static str {
                "application/json"
            }
        }
    };
}
