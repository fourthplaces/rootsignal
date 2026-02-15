use anyhow::Result;
use rootsignal_core::{ExtractedListings, ServerDeps};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::taxonomy::build_tag_instructions;

/// Page snapshot row (just the fields we need).
#[derive(Debug, sqlx::FromRow)]
struct PageSnapshot {
    id: Uuid,
    url: String,
    content: Option<String>,
    html: Option<String>,
}

/// Extraction row for returning IDs.
#[derive(Debug, sqlx::FromRow)]
struct ExtractionRow {
    id: Uuid,
}

/// Build the full system prompt by combining the config-driven preamble with dynamic taxonomy from the database.
async fn build_system_prompt(deps: &ServerDeps) -> Result<String> {
    let pool = deps.pool();
    let tag_instructions = build_tag_instructions("listing", pool).await?;
    Ok(deps.prompts.extraction_prompt(&tag_instructions))
}

/// Extract structured listings from a page_snapshot using AI.
pub async fn extract_from_snapshot(snapshot_id: Uuid, deps: &ServerDeps) -> Result<Vec<Uuid>> {
    let pool = deps.pool();

    // Mark as processing
    sqlx::query("UPDATE page_snapshots SET extraction_status = 'processing' WHERE id = $1")
        .bind(snapshot_id)
        .execute(pool)
        .await?;

    let snapshot = sqlx::query_as::<_, PageSnapshot>(
        "SELECT id, url, content, html FROM page_snapshots WHERE id = $1",
    )
    .bind(snapshot_id)
    .fetch_one(pool)
    .await?;

    let content = snapshot
        .content
        .as_deref()
        .or(snapshot.html.as_deref())
        .unwrap_or("");

    if content.is_empty() {
        sqlx::query(
            "UPDATE page_snapshots SET extraction_status = 'completed', extraction_completed_at = NOW() WHERE id = $1",
        )
        .bind(snapshot_id)
        .execute(pool)
        .await?;
        return Ok(vec![]);
    }

    // Truncate very long content
    let content = if content.len() > 30_000 {
        &content[..30_000]
    } else {
        content
    };

    // Build dynamic prompt from database taxonomy
    let system_prompt = build_system_prompt(deps).await?;

    let user_prompt = format!(
        "Extract community listings from this page (URL: {}):\n\n{}",
        snapshot.url, content
    );

    let model = &deps.file_config.models.extraction;

    // Use AI structured extraction
    let extracted: ExtractedListings = deps.ai.extract(model, &system_prompt, &user_prompt).await?;

    let mut extraction_ids = Vec::new();

    for listing in &extracted.listings {
        let data = serde_json::to_value(listing)?;

        // Fingerprint for dedup: hash of normalized key fields
        let fingerprint_input = format!(
            "{}:{}:{}:{}",
            listing.title.to_lowercase().trim(),
            listing
                .organization_name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .trim(),
            listing.start_time.as_deref().unwrap_or(""),
            listing.location_text.as_deref().unwrap_or(""),
        );
        let mut hasher = Sha256::new();
        hasher.update(fingerprint_input.as_bytes());
        let fingerprint = hasher.finalize().to_vec();

        let origin = serde_json::json!({
            "model": model,
            "snapshot_url": snapshot.url,
        });

        let row = sqlx::query_as::<_, ExtractionRow>(
            r#"
            INSERT INTO extractions (page_snapshot_id, fingerprint, schema_version, data, confidence_overall, confidence_ai, origin)
            VALUES ($1, $2, 1, $3, 0.7, 0.7, $4)
            ON CONFLICT (fingerprint, schema_version) DO UPDATE SET fingerprint = EXCLUDED.fingerprint
            RETURNING id
            "#,
        )
        .bind(snapshot_id)
        .bind(&fingerprint)
        .bind(&data)
        .bind(&origin)
        .fetch_one(pool)
        .await?;

        extraction_ids.push(row.id);
    }

    // Mark as completed
    sqlx::query(
        "UPDATE page_snapshots SET extraction_status = 'completed', extraction_completed_at = NOW() WHERE id = $1",
    )
    .bind(snapshot_id)
    .execute(pool)
    .await?;

    tracing::info!(
        snapshot_id = %snapshot_id,
        extractions = extraction_ids.len(),
        "Extraction complete"
    );

    Ok(extraction_ids)
}
