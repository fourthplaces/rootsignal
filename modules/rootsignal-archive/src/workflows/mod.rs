//! Restate durable workflows for archive operations.
//!
//! Currently houses the enrichment workflow that processes media files
//! through Claude vision (images) and OpenAI Whisper (video/audio).

pub mod enrichment;
pub mod types;

use sqlx::PgPool;

/// Shared dependency container for archive workflows.
///
/// Holds long-lived, cloneable resources needed by workflow implementations.
#[derive(Clone)]
pub struct ArchiveDeps {
    pub pg_pool: PgPool,
    pub anthropic_api_key: String,
    pub openai_api_key: String,
}

// ---------------------------------------------------------------------------
// Restate serde bridge macro
// ---------------------------------------------------------------------------

/// Implement Restate SDK serialization traits for types that already have serde derives.
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
