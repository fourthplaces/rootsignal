use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Source {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub name: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub consecutive_misses: i32,
    pub last_scraped_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub config: serde_json::Value,
    pub content_summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

// =============================================================================
// URL normalization and classification
// =============================================================================

/// Known tracking query parameters to strip from URLs.
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "fbclid",
    "gclid",
    "ref",
    "mc_cid",
    "mc_eid",
    "_ga",
    "_gl",
];

/// Social platform domains that get profile-style normalization.
const SOCIAL_DOMAINS: &[&str] = &["instagram.com", "facebook.com", "x.com", "tiktok.com"];

/// Canonicalize domain aliases to a single form.
fn canonical_domain(domain: &str) -> &str {
    match domain {
        "fb.com" | "m.facebook.com" => "facebook.com",
        "twitter.com" | "mobile.twitter.com" | "mobile.x.com" => "x.com",
        _ => domain,
    }
}

/// Extract the canonical domain from a URL string.
/// Returns the domain with www. stripped and aliases resolved.
fn extract_domain(url: &str) -> String {
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return String::new(),
    };
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    let domain = host.strip_prefix("www.").unwrap_or(&host);
    canonical_domain(domain).to_string()
}

/// Result of classifying and normalizing a raw user input.
pub struct ClassifiedSource {
    pub normalized_url: Option<String>,
    pub name: String,
    pub handle: Option<String>,
    pub config: serde_json::Value,
}

/// Classify and normalize a raw user input (URL or search query).
///
/// - Non-URL inputs become web searches (normalized_url = None).
/// - Social profile URLs are normalized to `https://{domain}/{handle}`.
/// - GoFundMe URLs preserve the `/f/{slug}` path.
/// - API URLs (usaspending, epa_echo) keep their query params.
/// - Generic websites strip tracking params and normalize.
/// - All URLs: lowercase domain, strip www., canonicalize aliases, force https://.
pub fn normalize_and_classify(input: &str) -> ClassifiedSource {
    let input = input.trim();

    // Not a URL → web_search
    if !input.starts_with("http://") && !input.starts_with("https://") {
        return ClassifiedSource {
            normalized_url: None,
            name: input.to_string(),
            handle: None,
            config: serde_json::json!({ "search_query": input, "max_results": 10 }),
        };
    }

    let parsed = match Url::parse(input) {
        Ok(u) => u,
        Err(_) => {
            return ClassifiedSource {
                normalized_url: Some(input.to_string()),
                name: input.to_string(),
                handle: None,
                config: serde_json::json!({}),
            };
        }
    };

    let host = parsed.host_str().unwrap_or("").to_lowercase();
    let domain = host.strip_prefix("www.").unwrap_or(&host);
    let domain = canonical_domain(domain);

    // Extract the first meaningful path segment (strip leading slashes and @)
    let path_segment = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .trim_start_matches('@')
        .to_string();

    // Social profiles: normalize to https://{domain}/{handle}
    if SOCIAL_DOMAINS.contains(&domain) {
        let handle = if path_segment.is_empty() {
            domain.to_string()
        } else {
            path_segment
        };
        let normalized = format!("https://{}/{}", domain, handle);
        return ClassifiedSource {
            normalized_url: Some(normalized),
            name: handle.clone(),
            handle: Some(handle),
            config: serde_json::json!({}),
        };
    }

    // GoFundMe search: /s?q={query}
    if domain == "gofundme.com" && path_segment == "s" {
        let query = parsed
            .query_pairs()
            .find(|(k, _)| k == "q")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let normalized = format!("https://gofundme.com/s?q={}", query);
        return ClassifiedSource {
            normalized_url: Some(normalized),
            name: format!("GoFundMe: {}", query),
            handle: None,
            config: serde_json::json!({ "search_query": query }),
        };
    }

    // GoFundMe campaign: preserve /f/{slug} path, strip query params
    if domain == "gofundme.com" {
        let name = if path_segment.is_empty() || path_segment == "f" {
            parsed
                .path()
                .trim_start_matches('/')
                .split('/')
                .nth(1)
                .unwrap_or("gofundme")
                .replace('-', " ")
        } else {
            path_segment.replace('-', " ")
        };
        let path = parsed.path().trim_end_matches('/');
        let normalized = format!("https://gofundme.com{}", path);
        return ClassifiedSource {
            normalized_url: Some(normalized),
            name,
            handle: None,
            config: serde_json::json!({}),
        };
    }

    // API sources: keep query params (they're meaningful), normalize scheme/host
    if domain == "api.usaspending.gov" {
        let normalized = rebuild_normalized_url(&parsed, domain, false);
        return ClassifiedSource {
            normalized_url: Some(normalized),
            name: format!("USAspending: {}", parsed.query().unwrap_or("all")),
            handle: None,
            config: serde_json::json!({ "api_url": input }),
        };
    }
    if domain == "echodata.epa.gov" {
        let normalized = rebuild_normalized_url(&parsed, domain, false);
        return ClassifiedSource {
            normalized_url: Some(normalized),
            name: format!("EPA ECHO: {}", parsed.query().unwrap_or("all")),
            handle: None,
            config: serde_json::json!({ "api_url": input }),
        };
    }

    // Generic website: strip tracking params, normalize
    let normalized = rebuild_normalized_url(&parsed, domain, true);
    ClassifiedSource {
        normalized_url: Some(normalized),
        name: domain.to_string(),
        handle: None,
        config: serde_json::json!({}),
    }
}

/// Rebuild a normalized URL from parsed components.
/// If `strip_tracking` is true, known tracking query params are removed.
fn rebuild_normalized_url(parsed: &Url, domain: &str, strip_tracking: bool) -> String {
    let path = parsed.path().trim_end_matches('/');

    let query = if strip_tracking {
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if pairs.is_empty() {
            None
        } else {
            let mut sorted = pairs;
            sorted.sort();
            Some(
                sorted
                    .iter()
                    .map(|(k, v)| {
                        if v.is_empty() {
                            k.clone()
                        } else {
                            format!("{}={}", k, v)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("&"),
            )
        }
    } else {
        parsed.query().map(String::from)
    };

    match query {
        Some(q) if !q.is_empty() => format!("https://{}{}?{}", domain, path, q),
        _ => format!("https://{}{}", domain, path),
    }
}

// =============================================================================
// Derived source properties (from URL)
// =============================================================================

/// Derive the source type from a URL.
/// This replaces the stored `source_type` column.
pub fn source_type_from_url(url: Option<&str>) -> &'static str {
    let url = match url {
        Some(u) => u,
        None => return "web_search",
    };
    let domain = extract_domain(url);
    match domain.as_str() {
        "instagram.com" => "instagram",
        "facebook.com" => "facebook",
        "x.com" => "x",
        "tiktok.com" => "tiktok",
        "gofundme.com" => {
            if url.contains("/s?q=") {
                "gofundme_search"
            } else {
                "gofundme"
            }
        }
        "api.usaspending.gov" => "usaspending",
        "echodata.epa.gov" => "epa_echo",
        _ => "website",
    }
}

/// Derive the source category from a URL for cadence computation.
pub fn source_category_from_url(url: Option<&str>) -> &'static str {
    match source_type_from_url(url) {
        "instagram" | "facebook" | "x" | "tiktok" | "gofundme" => "social",
        "gofundme_search" | "web_search" => "search",
        "usaspending" | "epa_echo" => "institutional",
        _ => "website",
    }
}

/// Derive the adapter name from a URL for scraping.
pub fn adapter_for_url(url: Option<&str>) -> &'static str {
    match source_type_from_url(url) {
        "instagram" => "apify_instagram",
        "facebook" => "apify_facebook",
        "x" => "apify_x",
        "tiktok" => "apify_tiktok",
        "gofundme" | "gofundme_search" => "apify_gofundme",
        _ => "spider",
    }
}

// =============================================================================
// Cadence computation
// =============================================================================

/// Compute the effective cadence in hours for a source.
///
/// - Base cadence: social=12h, search=24h, institutional=168h, website=168h
/// - Exponential backoff: `base * 2^misses`, capped at ceiling
/// - Ceilings: social=72h (3d), search=72h (3d), institutional=720h (30d), website=360h (15d)
pub fn compute_cadence(url: Option<&str>, consecutive_misses: i32) -> i32 {
    let category = source_category_from_url(url);

    let (base, ceiling): (i32, i32) = match category {
        "social" => (12, 72),
        "search" => (24, 72),
        "institutional" => (168, 720),
        _ => (168, 360),
    };

    let misses = consecutive_misses.min(30) as u32;
    let backoff = base.saturating_mul(1_i32.wrapping_shl(misses).max(1));
    backoff.min(ceiling)
}

// =============================================================================
// Source model
// =============================================================================

impl Source {
    /// Derive the source type from the URL.
    pub fn source_type(&self) -> &'static str {
        source_type_from_url(self.url.as_deref())
    }

    /// Convenience method to get the effective cadence for this source.
    pub fn effective_cadence_hours(&self) -> i32 {
        compute_cadence(self.url.as_deref(), self.consecutive_misses)
    }

    /// Create a source from a raw user input (URL or search query).
    ///
    /// Normalizes the URL, auto-detects type from domain, and deduplicates
    /// by returning an existing source if the normalized URL already exists.
    pub async fn create_from_input(input: &str, pool: &PgPool) -> Result<Self> {
        let classified = normalize_and_classify(input);

        // Dedup: if the normalized URL already exists, return the existing source
        if let Some(ref url) = classified.normalized_url {
            let existing = sqlx::query_as::<_, Self>("SELECT * FROM sources WHERE url = $1")
                .bind(url)
                .fetch_optional(pool)
                .await?;
            if let Some(source) = existing {
                return Ok(source);
            }
        }

        Self::create(
            &classified.name,
            classified.normalized_url.as_deref(),
            classified.handle.as_deref(),
            None,
            classified.config,
            pool,
        )
        .await
    }

    pub async fn create(
        name: &str,
        url: Option<&str>,
        handle: Option<&str>,
        entity_id: Option<Uuid>,
        config: serde_json::Value,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO sources (name, url, handle, entity_id, config, is_active)
            VALUES ($1, $2, $3, $4, $5, TRUE)
            RETURNING *
            "#,
        )
        .bind(name)
        .bind(url)
        .bind(handle)
        .bind(entity_id)
        .bind(config)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM sources WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_all(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM sources ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_active(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM sources WHERE is_active = TRUE ORDER BY last_scraped_at ASC NULLS FIRST",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_due_for_scrape(pool: &PgPool) -> Result<Vec<Self>> {
        let active = Self::find_active(pool).await?;
        let now = Utc::now();

        Ok(active
            .into_iter()
            .filter(|s| match s.last_scraped_at {
                None => true,
                Some(last) => {
                    let cadence = chrono::Duration::hours(s.effective_cadence_hours() as i64);
                    now - last >= cadence
                }
            })
            .collect())
    }

    /// Find an existing source by normalized URL, or create a new one.
    /// Returns `(source, was_created)`.
    pub async fn find_or_create_website(
        name: &str,
        url: &str,
        discovered_from: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<(Self, bool)> {
        let classified = normalize_and_classify(url);
        let normalized = classified.normalized_url.as_deref().unwrap_or(url);

        // Check if a source with this normalized URL already exists
        let existing = sqlx::query_as::<_, Self>("SELECT * FROM sources WHERE url = $1")
            .bind(normalized)
            .fetch_optional(pool)
            .await?;

        if let Some(source) = existing {
            return Ok((source, false));
        }

        // Create new source
        let mut config = serde_json::json!({});
        if let Some(parent_id) = discovered_from {
            config["discovered_from"] = serde_json::json!(parent_id.to_string());
        }

        let source = Self::create(name, Some(normalized), None, None, config, pool).await?;

        Ok((source, true))
    }

    /// Find an existing social source by normalized URL, or create a new one.
    /// Returns `(source, was_created)`.
    pub async fn find_or_create_social(
        platform: &str,
        handle: &str,
        _url: Option<&str>,
        entity_id: Uuid,
        pool: &PgPool,
    ) -> Result<(Self, bool)> {
        // Build canonical URL for this social profile
        let canonical_url = format!("https://{}.com/{}", platform, handle);

        // Check if a source with this URL already exists
        let existing = sqlx::query_as::<_, Self>("SELECT * FROM sources WHERE url = $1")
            .bind(&canonical_url)
            .fetch_optional(pool)
            .await?;

        if let Some(source) = existing {
            return Ok((source, false));
        }

        // Create new source
        let name = format!("{}@{}", handle, platform);
        let source = Self::create(
            &name,
            Some(&canonical_url),
            Some(handle),
            Some(entity_id),
            serde_json::json!({}),
            pool,
        )
        .await?;

        Ok((source, true))
    }

    pub async fn find_by_entity_id(entity_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM sources WHERE entity_id = $1 ORDER BY created_at DESC",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn set_entity_id(id: Uuid, entity_id: Uuid, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE sources SET entity_id = $2 WHERE id = $1")
            .bind(id)
            .bind(entity_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Record the result of a scrape. Resets or increments `consecutive_misses`
    /// and always updates `last_scraped_at`.
    pub async fn record_scrape_result(id: Uuid, had_content: bool, pool: &PgPool) -> Result<()> {
        if had_content {
            sqlx::query(
                "UPDATE sources SET consecutive_misses = 0, last_scraped_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(pool)
            .await?;
        } else {
            sqlx::query(
                "UPDATE sources SET consecutive_misses = consecutive_misses + 1, last_scraped_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn set_active_many(ids: &[Uuid], active: bool, pool: &PgPool) -> Result<u64> {
        let result = sqlx::query("UPDATE sources SET is_active = $2 WHERE id = ANY($1)")
            .bind(ids)
            .bind(active)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_many(ids: &[Uuid], pool: &PgPool) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sources WHERE id = ANY($1)")
            .bind(ids)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_and_classify ───────────────────────────────────────────

    #[test]
    fn test_classify_website() {
        let c = normalize_and_classify("https://example.com/some/page");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://example.com/some/page")
        );
        assert_eq!(c.name, "example.com");
        assert!(c.handle.is_none());
    }

    #[test]
    fn test_classify_website_strips_www() {
        let c = normalize_and_classify("https://www.example.com");
        assert_eq!(c.normalized_url.as_deref(), Some("https://example.com"));
        assert_eq!(c.name, "example.com");
    }

    #[test]
    fn test_classify_website_strips_tracking_params() {
        let c = normalize_and_classify("https://example.com/page?utm_source=foo&real=bar");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://example.com/page?real=bar")
        );
    }

    #[test]
    fn test_classify_website_strips_trailing_slash() {
        let c = normalize_and_classify("https://example.com/page/");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://example.com/page")
        );
    }

    #[test]
    fn test_classify_website_forces_https() {
        let c = normalize_and_classify("http://example.com/page");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://example.com/page")
        );
    }

    #[test]
    fn test_classify_website_lowercase_domain() {
        let c = normalize_and_classify("https://EXAMPLE.COM/Page");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://example.com/Page")
        );
        assert_eq!(c.name, "example.com");
    }

    #[test]
    fn test_classify_instagram_profile() {
        let c = normalize_and_classify("https://www.instagram.com/somecoffeeshop");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://instagram.com/somecoffeeshop")
        );
        assert_eq!(c.name, "somecoffeeshop");
        assert_eq!(c.handle.as_deref(), Some("somecoffeeshop"));
    }

    #[test]
    fn test_classify_instagram_strips_trailing_slash() {
        let c = normalize_and_classify("https://www.instagram.com/bri.anahata/");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://instagram.com/bri.anahata")
        );
    }

    #[test]
    fn test_classify_instagram_strips_query_params() {
        let c = normalize_and_classify("https://www.instagram.com/bri.anahata?blah=1");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://instagram.com/bri.anahata")
        );
    }

    #[test]
    fn test_classify_facebook() {
        let c = normalize_and_classify("https://facebook.com/somepage");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://facebook.com/somepage")
        );
        assert_eq!(c.name, "somepage");
    }

    #[test]
    fn test_classify_facebook_alias() {
        let c = normalize_and_classify("https://fb.com/somepage");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://facebook.com/somepage")
        );
    }

    #[test]
    fn test_classify_x() {
        let c = normalize_and_classify("https://x.com/someuser");
        assert_eq!(c.normalized_url.as_deref(), Some("https://x.com/someuser"));
        assert_eq!(c.name, "someuser");
        assert_eq!(c.handle.as_deref(), Some("someuser"));
    }

    #[test]
    fn test_classify_twitter_alias() {
        let c = normalize_and_classify("https://twitter.com/someuser");
        assert_eq!(c.normalized_url.as_deref(), Some("https://x.com/someuser"));
    }

    #[test]
    fn test_classify_tiktok() {
        let c = normalize_and_classify("https://tiktok.com/@someuser");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://tiktok.com/someuser")
        );
        assert_eq!(c.name, "someuser");
        assert_eq!(c.handle.as_deref(), Some("someuser"));
    }

    #[test]
    fn test_classify_gofundme_campaign() {
        let c = normalize_and_classify("https://www.gofundme.com/f/help-rebuild-community-center");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://gofundme.com/f/help-rebuild-community-center")
        );
        assert_eq!(c.name, "help rebuild community center");
        assert!(c.config.get("search_query").is_none());
    }

    #[test]
    fn test_classify_gofundme_search() {
        let c = normalize_and_classify("https://www.gofundme.com/s?q=minneapolis");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://gofundme.com/s?q=minneapolis")
        );
        assert_eq!(c.name, "GoFundMe: minneapolis");
        assert_eq!(c.config["search_query"], "minneapolis");
    }

    #[test]
    fn test_classify_gofundme_search_with_extra_params() {
        let c = normalize_and_classify("https://www.gofundme.com/s?q=community+garden&page=2");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://gofundme.com/s?q=community garden")
        );
        assert_eq!(c.config["search_query"], "community garden");
    }

    #[test]
    fn test_classify_gofundme_search_strips_www() {
        let c = normalize_and_classify("https://www.gofundme.com/s?q=fire+relief");
        assert!(c
            .normalized_url
            .as_deref()
            .unwrap()
            .starts_with("https://gofundme.com/s"));
    }

    #[test]
    fn test_classify_gofundme_search_empty_query() {
        let c = normalize_and_classify("https://www.gofundme.com/s?q=");
        assert_eq!(
            c.normalized_url.as_deref(),
            Some("https://gofundme.com/s?q=")
        );
        assert_eq!(c.name, "GoFundMe: ");
    }

    #[test]
    fn test_classify_usaspending() {
        let c = normalize_and_classify(
            "https://api.usaspending.gov/api/v2/search/spending_by_award/?recipient=GEO+Group",
        );
        assert!(c
            .normalized_url
            .as_deref()
            .unwrap()
            .starts_with("https://api.usaspending.gov"));
        assert!(c.name.starts_with("USAspending:"));
    }

    #[test]
    fn test_classify_epa_echo() {
        let c = normalize_and_classify(
            "https://echodata.epa.gov/echo/dfr_rest_services.get_facility_info?p_name=Acme",
        );
        assert!(c
            .normalized_url
            .as_deref()
            .unwrap()
            .starts_with("https://echodata.epa.gov"));
        assert!(c.name.starts_with("EPA ECHO:"));
    }

    #[test]
    fn test_classify_search_query() {
        let c = normalize_and_classify("third places community spaces Minneapolis");
        assert!(c.normalized_url.is_none());
        assert_eq!(c.name, "third places community spaces Minneapolis");
        assert_eq!(
            c.config["search_query"],
            "third places community spaces Minneapolis"
        );
        assert_eq!(c.config["max_results"], 10);
    }

    #[test]
    fn test_classify_whitespace_trimmed() {
        let c = normalize_and_classify("  https://example.com  ");
        assert_eq!(c.normalized_url.as_deref(), Some("https://example.com"));
    }

    // ── source_type_from_url ────────────────────────────────────────────

    #[test]
    fn test_source_type_from_url() {
        assert_eq!(source_type_from_url(None), "web_search");
        assert_eq!(
            source_type_from_url(Some("https://instagram.com/user")),
            "instagram"
        );
        assert_eq!(
            source_type_from_url(Some("https://facebook.com/page")),
            "facebook"
        );
        assert_eq!(source_type_from_url(Some("https://x.com/user")), "x");
        assert_eq!(
            source_type_from_url(Some("https://tiktok.com/user")),
            "tiktok"
        );
        assert_eq!(
            source_type_from_url(Some("https://gofundme.com/f/slug")),
            "gofundme"
        );
        assert_eq!(
            source_type_from_url(Some("https://gofundme.com/s?q=minneapolis")),
            "gofundme_search"
        );
        assert_eq!(
            source_type_from_url(Some("https://api.usaspending.gov/api/v2")),
            "usaspending"
        );
        assert_eq!(
            source_type_from_url(Some("https://echodata.epa.gov/echo")),
            "epa_echo"
        );
        assert_eq!(source_type_from_url(Some("https://example.com")), "website");
    }

    // ── adapter_for_url ─────────────────────────────────────────────────

    #[test]
    fn test_adapter_for_url() {
        assert_eq!(
            adapter_for_url(Some("https://instagram.com/user")),
            "apify_instagram"
        );
        assert_eq!(
            adapter_for_url(Some("https://facebook.com/page")),
            "apify_facebook"
        );
        assert_eq!(adapter_for_url(Some("https://x.com/user")), "apify_x");
        assert_eq!(
            adapter_for_url(Some("https://tiktok.com/user")),
            "apify_tiktok"
        );
        assert_eq!(
            adapter_for_url(Some("https://gofundme.com/f/slug")),
            "apify_gofundme"
        );
        assert_eq!(
            adapter_for_url(Some("https://gofundme.com/s?q=minneapolis")),
            "apify_gofundme"
        );
        assert_eq!(adapter_for_url(Some("https://example.com")), "spider");
    }

    // ── compute_cadence ─────────────────────────────────────────────────

    #[test]
    fn test_compute_cadence_base_cases() {
        assert_eq!(compute_cadence(Some("https://example.com"), 0), 168);
        assert_eq!(compute_cadence(None, 0), 24); // web_search
        assert_eq!(compute_cadence(Some("https://instagram.com/u"), 0), 12);
        assert_eq!(compute_cadence(Some("https://facebook.com/p"), 0), 12);
        assert_eq!(compute_cadence(Some("https://x.com/u"), 0), 12);
        assert_eq!(compute_cadence(Some("https://tiktok.com/u"), 0), 12);
        assert_eq!(compute_cadence(Some("https://gofundme.com/f/s"), 0), 12);
        assert_eq!(
            compute_cadence(Some("https://gofundme.com/s?q=mpls"), 0),
            24
        ); // gofundme_search = search cadence
        assert_eq!(
            compute_cadence(Some("https://api.usaspending.gov/api"), 0),
            168
        );
        assert_eq!(
            compute_cadence(Some("https://echodata.epa.gov/echo"), 0),
            168
        );
    }

    #[test]
    fn test_compute_cadence_backoff() {
        // social: base=12, ceiling=72
        assert_eq!(compute_cadence(Some("https://instagram.com/u"), 1), 24);
        assert_eq!(compute_cadence(Some("https://instagram.com/u"), 2), 48);
        assert_eq!(compute_cadence(Some("https://instagram.com/u"), 3), 72);

        // search: base=24, ceiling=72
        assert_eq!(compute_cadence(None, 1), 48);
        assert_eq!(compute_cadence(None, 2), 72);

        // website: base=168, ceiling=360
        assert_eq!(compute_cadence(Some("https://example.com"), 1), 336);
        assert_eq!(compute_cadence(Some("https://example.com"), 2), 360);

        // institutional: base=168, ceiling=720
        assert_eq!(
            compute_cadence(Some("https://api.usaspending.gov/api"), 1),
            336
        );
        assert_eq!(
            compute_cadence(Some("https://api.usaspending.gov/api"), 2),
            672
        );
        assert_eq!(
            compute_cadence(Some("https://api.usaspending.gov/api"), 3),
            720
        );
    }

    #[test]
    fn test_compute_cadence_ceilings() {
        assert_eq!(compute_cadence(Some("https://instagram.com/u"), 10), 72);
        assert_eq!(compute_cadence(None, 10), 72);
        assert_eq!(compute_cadence(Some("https://example.com"), 10), 360);
        assert_eq!(
            compute_cadence(Some("https://api.usaspending.gov/api"), 10),
            720
        );
    }

    // ── URL normalization dedup scenarios ────────────────────────────────

    #[test]
    fn test_normalization_dedup_instagram_variants() {
        let a = normalize_and_classify("https://www.instagram.com/bri.anahata/");
        let b = normalize_and_classify("https://instagram.com/bri.anahata");
        let c = normalize_and_classify("https://www.instagram.com/bri.anahata?igsh=abc");
        assert_eq!(a.normalized_url, b.normalized_url);
        assert_eq!(b.normalized_url, c.normalized_url);
    }

    #[test]
    fn test_normalization_dedup_facebook_aliases() {
        let a = normalize_and_classify("https://fb.com/somepage");
        let b = normalize_and_classify("https://facebook.com/somepage");
        let c = normalize_and_classify("https://www.facebook.com/somepage/");
        assert_eq!(a.normalized_url, b.normalized_url);
        assert_eq!(b.normalized_url, c.normalized_url);
    }

    #[test]
    fn test_normalization_dedup_twitter_aliases() {
        let a = normalize_and_classify("https://twitter.com/someuser");
        let b = normalize_and_classify("https://x.com/someuser");
        assert_eq!(a.normalized_url, b.normalized_url);
    }

    #[test]
    fn test_normalization_dedup_website_tracking_params() {
        let a = normalize_and_classify("https://example.com/page?utm_source=google&real=yes");
        let b = normalize_and_classify("https://example.com/page?real=yes");
        assert_eq!(a.normalized_url, b.normalized_url);
    }

    #[test]
    fn test_normalization_dedup_gofundme_search_variants() {
        let a = normalize_and_classify("https://www.gofundme.com/s?q=minneapolis");
        let b = normalize_and_classify("https://gofundme.com/s?q=minneapolis");
        assert_eq!(a.normalized_url, b.normalized_url);
    }

    #[test]
    fn test_gofundme_search_vs_campaign_different_types() {
        let search = normalize_and_classify("https://gofundme.com/s?q=minneapolis");
        let campaign = normalize_and_classify("https://gofundme.com/f/help-minneapolis");
        assert_ne!(search.normalized_url, campaign.normalized_url);
        assert_eq!(
            source_type_from_url(search.normalized_url.as_deref()),
            "gofundme_search"
        );
        assert_eq!(
            source_type_from_url(campaign.normalized_url.as_deref()),
            "gofundme"
        );
    }

    #[test]
    fn test_gofundme_search_category_is_search() {
        assert_eq!(
            source_category_from_url(Some("https://gofundme.com/s?q=minneapolis")),
            "search"
        );
        assert_eq!(
            source_category_from_url(Some("https://gofundme.com/f/some-campaign")),
            "social"
        );
    }
}
