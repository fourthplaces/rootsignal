use anyhow::Result;
use chrono::DateTime;
use pgvector::Vector;
use rootsignal_core::{ExtractedSignals, ServerDeps};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::Entity;
use crate::geo::{Location, Locationable};
use crate::search::Embedding;
use crate::shared::Schedule;

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

/// Extract structured signals from a page_snapshot using AI.
pub async fn extract_signals_from_snapshot(
    snapshot_id: Uuid,
    deps: &ServerDeps,
) -> Result<Vec<Uuid>> {
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

    let system_prompt = deps.prompts.signal_extraction_prompt();
    let user_prompt = format!(
        "Extract signals from this page (URL: {}):\n\n{}",
        snapshot.url, content
    );

    let model = &deps.file_config.models.extraction;

    let extracted: ExtractedSignals = deps.ai.extract(model, system_prompt, &user_prompt).await?;

    let mut signal_ids = Vec::new();

    for signal in &extracted.signals {
        // Fingerprint for dedup: hash of normalized key fields
        let fingerprint_input = format!(
            "{}:{}:{}:{}",
            signal.signal_type.to_lowercase().trim(),
            signal.content.to_lowercase().trim(),
            signal
                .entity_name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .trim(),
            signal.about.as_deref().unwrap_or("").to_lowercase().trim(),
        );
        let mut hasher = Sha256::new();
        hasher.update(fingerprint_input.as_bytes());
        let fingerprint = hasher.finalize().to_vec();

        let in_language = signal.source_locale.as_deref().unwrap_or("en");

        // Resolve entity if mentioned
        let entity_id = if let Some(ref entity_name) = signal.entity_name {
            let entity_type = signal.entity_type.as_deref().unwrap_or("organization");
            let entity = Entity::find_or_create(entity_name, entity_type, None, None, pool).await?;
            Some(entity.id)
        } else {
            None
        };

        // Create extraction record (provenance)
        let data = serde_json::to_value(signal)?;
        let origin = serde_json::json!({
            "model": model,
            "snapshot_url": snapshot.url,
        });

        let extraction = sqlx::query_as::<_, ExtractionRow>(
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

        // Create signal row
        let signal_row = super::super::models::signal::Signal::create(
            &signal.signal_type,
            &signal.content,
            signal.about.as_deref(),
            entity_id,
            signal.source_url.as_deref().or(Some(&snapshot.url)),
            Some(snapshot_id),
            Some(extraction.id),
            None, // institutional_source (community extraction)
            None, // institutional_record_id
            None, // source_citation_url
            0.7,
            &fingerprint,
            in_language,
            pool,
        )
        .await?;

        // Normalize into polymorphic tables:

        // 1. Location → locationables (locatable_type = 'signal')
        if signal.city.is_some() || signal.state.is_some() || signal.postal_code.is_some() {
            let location = Location::find_or_create_from_extraction(
                signal.city.as_deref(),
                signal.state.as_deref(),
                signal.postal_code.as_deref(),
                signal.address.as_deref(),
                pool,
            )
            .await?;
            Locationable::create(location.id, "signal", signal_row.id, true, pool).await?;
        }

        // 2. Schedule → schedules (scheduleable_type = 'signal')
        if signal.start_date.is_some() || signal.is_recurring == Some(true) {
            let valid_from = signal
                .start_date
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc));
            let valid_through = signal
                .end_date
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc));

            Schedule::create(
                "signal",
                signal_row.id,
                signal.start_date.as_deref(), // dtstart
                None,                          // repeat_frequency
                None,                          // byday
                None,                          // bymonthday
                signal.recurrence_description.as_deref(),
                valid_from,
                valid_through,
                None, // opens_at
                None, // closes_at
                pool,
            )
            .await?;
        }

        // 3. Embedding → embeddings (embeddable_type = 'signal')
        let embed_text = format!(
            "{} {}",
            signal.content,
            signal.about.as_deref().unwrap_or("")
        );
        let mut embed_hasher = Sha256::new();
        embed_hasher.update(embed_text.as_bytes());
        let embed_hash = hex::encode(embed_hasher.finalize());

        match deps.embedding_service.embed(&embed_text).await {
            Ok(raw_embedding) => {
                let vector = Vector::from(raw_embedding);
                Embedding::upsert("signal", signal_row.id, in_language, vector, &embed_hash, pool)
                    .await?;
            }
            Err(e) => {
                tracing::warn!(signal_id = %signal_row.id, error = %e, "Failed to embed signal (non-fatal)");
            }
        }

        signal_ids.push(signal_row.id);
    }

    // Mark snapshot as completed
    sqlx::query(
        "UPDATE page_snapshots SET extraction_status = 'completed', extraction_completed_at = NOW() WHERE id = $1",
    )
    .bind(snapshot_id)
    .execute(pool)
    .await?;

    tracing::info!(
        snapshot_id = %snapshot_id,
        signals = signal_ids.len(),
        "Signal extraction complete"
    );

    Ok(signal_ids)
}
