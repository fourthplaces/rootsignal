use anyhow::{Context, Result};
use pgvector::Vector;
use sha2::{Digest, Sha256};
use taproot_core::ServerDeps;
use uuid::Uuid;

use crate::entities::{Embedding, Translation};

/// Compose the text to embed for a given record type.
/// Always uses English text — from translations if source is non-English, from source fields if English.
async fn compose_embedding_text(
    record_type: &str,
    record_id: Uuid,
    source_locale: &str,
    deps: &ServerDeps,
) -> Result<Option<String>> {
    let pool = deps.pool();

    if source_locale == "en" {
        // Fetch directly from source fields
        match record_type {
            "listing" => {
                let row = sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT title, description FROM listings WHERE id = $1",
                )
                .bind(record_id)
                .fetch_one(pool)
                .await?;
                let text = match row.1 {
                    Some(desc) => format!("{} {}", row.0, desc),
                    None => row.0,
                };
                Ok(Some(text))
            }
            "entity" => {
                let row = sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT name, description FROM entities WHERE id = $1",
                )
                .bind(record_id)
                .fetch_one(pool)
                .await?;
                let text = match row.1 {
                    Some(desc) => format!("{} {}", row.0, desc),
                    None => row.0,
                };
                Ok(Some(text))
            }
            "service" => {
                let row = sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT name, description FROM services WHERE id = $1",
                )
                .bind(record_id)
                .fetch_one(pool)
                .await?;
                let text = match row.1 {
                    Some(desc) => format!("{} {}", row.0, desc),
                    None => row.0,
                };
                Ok(Some(text))
            }
            _ => Ok(None),
        }
    } else {
        // Fetch English translations
        let translations =
            Translation::find_for(record_type, record_id, "en", pool).await?;

        let (title_fields, desc_fields) = match record_type {
            "listing" => ("title", "description"),
            "entity" => ("name", "description"),
            "service" => ("name", "description"),
            _ => return Ok(None),
        };

        let title = translations
            .iter()
            .find(|t| t.field_name == title_fields)
            .map(|t| t.content.as_str());
        let desc = translations
            .iter()
            .find(|t| t.field_name == desc_fields)
            .map(|t| t.content.as_str());

        match (title, desc) {
            (Some(t), Some(d)) => Ok(Some(format!("{} {}", t, d))),
            (Some(t), None) => Ok(Some(t.to_string())),
            _ => Ok(None),
        }
    }
}

/// Generate and store an English embedding for a record.
/// Returns the embedding ID if successful, None if no embeddable text.
pub async fn generate_embedding(
    record_type: &str,
    record_id: Uuid,
    source_locale: &str,
    deps: &ServerDeps,
) -> Result<Option<Uuid>> {
    let text = compose_embedding_text(record_type, record_id, source_locale, deps)
        .await
        .context("compose embedding text")?;

    let text = match text {
        Some(t) if !t.trim().is_empty() => t,
        _ => {
            tracing::warn!(
                record_type = record_type,
                record_id = %record_id,
                "No text to embed"
            );
            return Ok(None);
        }
    };

    // Hash the text to check if re-embedding is needed
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let source_text_hash = hex::encode(hasher.finalize());

    // Skip if hash unchanged
    if let Some(existing_hash) =
        Embedding::get_hash(record_type, record_id, "en", deps.pool()).await?
    {
        if existing_hash == source_text_hash {
            tracing::debug!(
                record_type = record_type,
                record_id = %record_id,
                "Embedding unchanged, skipping"
            );
            return Ok(None);
        }
    }

    let raw_embedding = deps
        .embedding_service
        .embed(&text)
        .await
        .context("generate embedding vector")?;

    let vector = Vector::from(raw_embedding);

    let embedding = Embedding::upsert(
        record_type,
        record_id,
        "en",
        vector,
        &source_text_hash,
        deps.pool(),
    )
    .await?;

    tracing::info!(
        record_type = record_type,
        record_id = %record_id,
        "Generated English embedding"
    );

    Ok(Some(embedding.id))
}
