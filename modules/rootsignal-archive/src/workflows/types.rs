//! Request/response types for archive workflows.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Input for the enrichment workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentRequest {
    pub files: Vec<EnrichmentFileRequest>,
}

/// A single file to enrich within the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentFileRequest {
    pub file_id: Uuid,
    pub mime_type: String,
    /// Base64-encoded media bytes.
    pub media_bytes_b64: String,
}

/// Result of the enrichment workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentResult {
    pub files_enriched: u32,
    pub files_failed: u32,
}

/// Empty request for `get_status` shared handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest;

crate::impl_restate_serde!(EnrichmentRequest);
crate::impl_restate_serde!(EnrichmentResult);
crate::impl_restate_serde!(EmptyRequest);
