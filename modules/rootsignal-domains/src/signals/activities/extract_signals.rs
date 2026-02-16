use std::collections::HashMap;

use anyhow::Result;
use chrono::DateTime;
use pgvector::Vector;
use rootsignal_core::{ExtractedSignals, ServerDeps};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::geo::{Location, Locationable};
use crate::search::Embedding;
use crate::shared::Schedule;
use crate::signals::models::signal::Signal;

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

/// Build the alias map and prompt context from existing signals for this source.
fn build_signal_context(existing_signals: &[Signal]) -> (HashMap<String, Uuid>, String) {
    let mut alias_map = HashMap::new();
    let mut context_lines = Vec::new();

    for (i, signal) in existing_signals.iter().enumerate() {
        let alias = format!("signal_{}", i + 1);
        alias_map.insert(alias.clone(), signal.id);

        let about_str = signal
            .about
            .as_deref()
            .map(|a| format!(" (about: \"{}\")", a))
            .unwrap_or_default();

        let date_str = signal
            .broadcasted_at
            .map(|dt| format!(" — {}", dt.format("%Y-%m-%d")))
            .unwrap_or_default();

        context_lines.push(format!(
            "{}: [{}] \"{}\"{}{}\n",
            alias, signal.signal_type, signal.content, about_str, date_str,
        ));
    }

    (alias_map, context_lines.join(""))
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
        "SELECT id, url, raw_content AS content, html FROM page_snapshots WHERE id = $1",
    )
    .bind(snapshot_id)
    .fetch_one(pool)
    .await?;

    // Resolve source_id and entity_id from the source chain
    let (source_id, entity_id): (Option<Uuid>, Option<Uuid>) = sqlx::query_as::<_, (Option<Uuid>, Option<Uuid>)>(
        r#"
        SELECT ds.source_id, src.entity_id FROM domain_snapshots ds
        JOIN sources src ON src.id = ds.source_id
        WHERE ds.page_snapshot_id = $1
        LIMIT 1
        "#,
    )
    .bind(snapshot_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or((None, None));

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

    // Fetch existing signals for this source (for LLM-driven matching)
    let existing_signals = if let Some(sid) = source_id {
        Signal::find_by_source(sid, 50, 0, pool).await?
    } else {
        vec![]
    };
    let (alias_map, signals_context) = build_signal_context(&existing_signals);

    let system_prompt = deps.prompts.signal_extraction_prompt();

    let user_prompt = if alias_map.is_empty() {
        format!(
            "Extract signals from this page (URL: {}):\n\n{}",
            snapshot.url, content
        )
    } else {
        format!(
            "Extract signals from this page (URL: {}):\n\n{}\n\n## Previously Known Signals\n\n{}",
            snapshot.url, content, signals_context
        )
    };

    let model = &deps.file_config.models.extraction;

    let extracted: ExtractedSignals = deps.ai.extract(model, system_prompt, &user_prompt).await?;

    let mut signal_ids = Vec::new();

    for signal in &extracted.signals {
        let in_language = signal.source_locale.as_deref().unwrap_or("en");

        // Create extraction record (provenance)
        let data = serde_json::to_value(signal)?;
        let origin = serde_json::json!({
            "model": model,
            "snapshot_url": snapshot.url,
        });

        let extraction = sqlx::query_as::<_, ExtractionRow>(
            r#"
            INSERT INTO extractions (page_snapshot_id, schema_version, data, confidence_overall, confidence_ai, origin)
            VALUES ($1, 1, $2, 0.7, 0.7, $3)
            RETURNING id
            "#,
        )
        .bind(snapshot_id)
        .bind(&data)
        .bind(&origin)
        .fetch_one(pool)
        .await?;

        // Parse broadcasted_at if the LLM extracted one
        let broadcasted_at = signal
            .broadcasted_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        // Determine if this is an UPDATE of an existing signal or a new INSERT
        let is_update = signal
            .existing_signal_alias
            .as_deref()
            .and_then(|alias| alias_map.get(alias));

        let signal_row = if let Some(&existing_id) = is_update {
            // UPDATE: refresh existing signal with new extraction data
            // First, clean up polymorphic records that will be recreated
            Locationable::delete_for("signal", existing_id, pool).await?;
            Schedule::delete_for("signal", existing_id, pool).await?;

            Signal::update_from_extraction(
                existing_id,
                &signal.signal_type,
                &signal.content,
                signal.about.as_deref(),
                entity_id,
                signal.source_url.as_deref().or(Some(&snapshot.url)),
                Some(snapshot_id),
                Some(extraction.id),
                0.7,
                broadcasted_at,
                pool,
            )
            .await?
        } else {
            // INSERT: new signal
            Signal::insert(
                &signal.signal_type,
                &signal.content,
                signal.about.as_deref(),
                entity_id,
                signal.source_url.as_deref().or(Some(&snapshot.url)),
                Some(snapshot_id),
                Some(extraction.id),
                None, // source_citation_url
                0.7,
                in_language,
                broadcasted_at,
                pool,
            )
            .await?
        };

        // Flag for investigation if the LLM detected deeper phenomenon
        if signal.needs_investigation == Some(true) {
            sqlx::query(
                "UPDATE signals SET needs_investigation = true, investigation_reason = $1 WHERE id = $2",
            )
            .bind(signal.investigation_reason.as_deref())
            .bind(signal_row.id)
            .execute(pool)
            .await?;
        }

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

    let matched_count = extracted
        .signals
        .iter()
        .filter(|s| {
            s.existing_signal_alias
                .as_deref()
                .and_then(|a| alias_map.get(a))
                .is_some()
        })
        .count();

    tracing::info!(
        snapshot_id = %snapshot_id,
        signals = signal_ids.len(),
        existing_shown = alias_map.len(),
        existing_matched = matched_count,
        "Signal extraction complete"
    );

    Ok(signal_ids)
}
