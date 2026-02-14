use axum::http::HeaderMap;

/// Locale newtype for request-scoped context in GraphQL resolvers.
pub struct Locale(pub String);

/// Extract locale using precedence: explicit arg > Accept-Language header > "en" default.
pub fn extract_locale(headers: &HeaderMap, explicit: Option<&str>, supported_locales: &[String]) -> Locale {
    let supported: Vec<&str> = supported_locales.iter().map(|s| s.as_str()).collect();

    if let Some(locale) = explicit {
        if supported.contains(&locale) {
            return Locale(locale.to_string());
        }
    }

    if let Some(header) = headers.get("accept-language").and_then(|v| v.to_str().ok()) {
        let primary = header.split(',').next().unwrap_or("").trim();
        let lang = primary.split(';').next().unwrap_or("").trim();
        if supported.contains(&lang) {
            return Locale(lang.to_string());
        }
    }

    Locale("en".to_string())
}
