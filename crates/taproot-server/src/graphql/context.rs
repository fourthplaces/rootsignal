use axum::http::HeaderMap;

/// Locale newtype for request-scoped context in GraphQL resolvers.
pub struct Locale(pub String);

const SUPPORTED_LOCALES: &[&str] = &["en", "es", "so", "ht"];

/// Extract locale using precedence: explicit arg > Accept-Language header > "en" default.
pub fn extract_locale(headers: &HeaderMap, explicit: Option<&str>) -> Locale {
    if let Some(locale) = explicit {
        if SUPPORTED_LOCALES.contains(&locale) {
            return Locale(locale.to_string());
        }
    }

    if let Some(header) = headers.get("accept-language").and_then(|v| v.to_str().ok()) {
        let primary = header.split(',').next().unwrap_or("").trim();
        let lang = primary.split(';').next().unwrap_or("").trim();
        if SUPPORTED_LOCALES.contains(&lang) {
            return Locale(lang.to_string());
        }
    }

    Locale("en".to_string())
}
