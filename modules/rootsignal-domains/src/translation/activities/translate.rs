use anyhow::{Context, Result};
use rootsignal_core::ServerDeps;
use uuid::Uuid;

use crate::entities::Translation;

/// Supported locales (default; prefer config.supported_locales when available).
pub const ALL_LOCALES: &[&str] = &["en", "es", "so", "ht"];

/// Fields to translate per record type.
fn translatable_fields(record_type: &str) -> &'static [&'static str] {
    match record_type {
        "listing" => &["title", "description"],
        "entity" => &["description"],
        "service" => &[
            "name",
            "description",
            "eligibility_description",
            "fees_description",
            "application_process",
        ],
        "tag" => &["display_name"],
        _ => &[],
    }
}

/// Fetch source field values for a translatable record.
async fn fetch_source_fields(
    record_type: &str,
    record_id: Uuid,
    deps: &ServerDeps,
) -> Result<Vec<(String, String)>> {
    let pool = deps.pool();
    let fields = translatable_fields(record_type);
    let mut result = Vec::new();

    match record_type {
        "listing" => {
            let row = sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT title, description FROM listings WHERE id = $1",
            )
            .bind(record_id)
            .fetch_one(pool)
            .await
            .context("fetch listing for translation")?;

            result.push(("title".to_string(), row.0));
            if let Some(desc) = row.1 {
                result.push(("description".to_string(), desc));
            }
        }
        "entity" => {
            let row = sqlx::query_as::<_, (Option<String>,)>(
                "SELECT description FROM entities WHERE id = $1",
            )
            .bind(record_id)
            .fetch_one(pool)
            .await
            .context("fetch entity for translation")?;

            if let Some(desc) = row.0 {
                result.push(("description".to_string(), desc));
            }
        }
        "service" => {
            let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>)>(
                "SELECT name, description, eligibility_description, fees_description, application_process FROM services WHERE id = $1",
            )
            .bind(record_id)
            .fetch_one(pool)
            .await
            .context("fetch service for translation")?;

            result.push(("name".to_string(), row.0));
            if let Some(v) = row.1 {
                result.push(("description".to_string(), v));
            }
            if let Some(v) = row.2 {
                result.push(("eligibility_description".to_string(), v));
            }
            if let Some(v) = row.3 {
                result.push(("fees_description".to_string(), v));
            }
            if let Some(v) = row.4 {
                result.push(("application_process".to_string(), v));
            }
        }
        "tag" => {
            let row = sqlx::query_as::<_, (Option<String>,)>(
                "SELECT display_name FROM tags WHERE id = $1",
            )
            .bind(record_id)
            .fetch_one(pool)
            .await
            .context("fetch tag for translation")?;

            if let Some(dn) = row.0 {
                result.push(("display_name".to_string(), dn));
            }
        }
        _ => {}
    }

    // Filter to only fields in the translatable set
    let field_set: Vec<&str> = fields.to_vec();
    result.retain(|(name, _)| field_set.contains(&name.as_str()));

    Ok(result)
}

/// Fetch field values from existing translations for a given locale.
async fn fetch_translated_fields(
    record_type: &str,
    record_id: Uuid,
    locale: &str,
    deps: &ServerDeps,
) -> Result<Vec<(String, String)>> {
    let translations = Translation::find_for(record_type, record_id, locale, deps.pool()).await?;
    Ok(translations
        .into_iter()
        .map(|t| (t.field_name, t.content))
        .collect())
}

/// Translate a record's fields to a target locale using AI.
///
/// `source_locale` here means "translate FROM this locale". If translating from the
/// record's original language, we fetch from DB source fields. If translating from
/// an intermediate locale (e.g., English translations), we fetch from the translations table.
pub async fn translate_record(
    record_type: &str,
    record_id: Uuid,
    source_locale: &str,
    target_locale: &str,
    deps: &ServerDeps,
) -> Result<Vec<Uuid>> {
    // Determine where to get source text:
    // - If source_locale matches the record's actual source_locale, fetch from DB fields
    // - Otherwise, fetch from translations table (e.g., English translations as intermediate)
    // Tags don't have source_locale — they're always seeded in English
    let record_source_locale = if record_type == "tag" {
        "en".to_string()
    } else {
        sqlx::query_as::<_, (String,)>(
            &format!("SELECT source_locale FROM {}s WHERE id = $1", record_type),
        )
        .bind(record_id)
        .fetch_optional(deps.pool())
        .await?
        .map(|r| r.0)
        .unwrap_or_else(|| "en".to_string())
    };

    let fields = if source_locale == record_source_locale {
        // Translating from the original language — read source fields
        fetch_source_fields(record_type, record_id, deps).await?
    } else {
        // Translating from an intermediate language (e.g., English) — read translations
        fetch_translated_fields(record_type, record_id, source_locale, deps).await?
    };

    if fields.is_empty() {
        return Ok(vec![]);
    }

    let locale_name = match target_locale {
        "en" => "English",
        "es" => "Spanish",
        "so" => "Somali",
        "ht" => "Haitian Creole",
        _ => "English",
    };

    let source_locale_name = match source_locale {
        "en" => "English",
        "es" => "Spanish",
        "so" => "Somali",
        "ht" => "Haitian Creole",
        _ => "English",
    };

    let mut translation_ids = Vec::new();

    for (field_name, content) in &fields {
        let system_prompt = format!(
            "You are a professional translator for community services. \
             Translate the following text from {} to {}. \
             Return ONLY the translated text, nothing else. \
             Preserve any proper nouns, addresses, phone numbers, and URLs as-is. \
             Keep the same tone and formality level as the original.",
            source_locale_name, locale_name,
        );

        let translated = deps
            .ai
            .chat_completion(&system_prompt, content)
            .await
            .context(format!(
                "translate {} field '{}' to {}",
                record_type, field_name, target_locale
            ))?;

        let translation = Translation::create(
            record_type,
            record_id,
            field_name,
            target_locale,
            translated.trim(),
            Some(source_locale),
            Some("gpt-4o"),
            deps.pool(),
        )
        .await?;

        translation_ids.push(translation.id);
    }

    tracing::info!(
        record_type = record_type,
        record_id = %record_id,
        target_locale = target_locale,
        fields = translation_ids.len(),
        "Translated record"
    );

    Ok(translation_ids)
}
