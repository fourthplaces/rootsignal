use anyhow::Result;
use taproot_core::ServerDeps;

/// Translate a search query to English for indexing/search.
///
/// Uses AI chat_completion matching the pattern in translation/activities/translate.rs.
/// Returns the original query unchanged if translation fails or locale is English.
pub async fn translate_query_to_english(
    query: &str,
    source_locale: &str,
    deps: &ServerDeps,
) -> Result<String> {
    if source_locale == "en" {
        return Ok(query.to_string());
    }

    let locale_name = match source_locale {
        "es" => "Spanish",
        "so" => "Somali",
        "ht" => "Haitian Creole",
        _ => return Ok(query.to_string()),
    };

    let system_prompt = format!(
        "You are a translator for community service search queries. \
         Translate the following {} search query to English. \
         Return ONLY the translated text, nothing else. \
         Preserve any proper nouns, addresses, and place names as-is.",
        locale_name,
    );

    match deps.ai.chat_completion(&system_prompt, query).await {
        Ok(translated) => Ok(translated.trim().to_string()),
        Err(e) => {
            tracing::warn!(
                error = %e,
                source_locale = source_locale,
                "Query translation failed, using original"
            );
            Ok(query.to_string())
        }
    }
}
