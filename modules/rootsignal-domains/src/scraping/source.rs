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
    pub source_type: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub consecutive_misses: i32,
    pub last_scraped_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub config: serde_json::Value,
    pub qualification_status: String,
    pub qualification_summary: Option<String>,
    pub qualification_score: Option<i32>,
    pub content_summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Map platform-specific source types to a scheduling category.
fn source_category(source_type: &str) -> &'static str {
    match source_type {
        "instagram" | "facebook" | "x" | "tiktok" | "gofundme" => "social",
        "web_search" | "search_query" => "search",
        "usaspending" | "epa_echo" => "institutional",
        _ => "website",
    }
}

/// Compute the effective cadence in hours for a source.
///
/// - Base cadence: social=12h, search=24h, institutional=168h, website=168h
/// - Exponential backoff: `base * 2^misses`, capped at ceiling
/// - Ceilings: social=72h (3d), search=72h (3d), institutional=720h (30d), website=360h (15d)
/// - Qualification gate removed — adaptive cadence handles source quality mechanically.
pub fn compute_cadence(source_type: &str, consecutive_misses: i32) -> i32 {
    let category = source_category(source_type);

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

/// Result of parsing a raw user input (URL or search query) into source fields.
struct ParsedInput {
    name: String,
    source_type: String,
    url: Option<String>,
    handle: Option<String>,
    config: serde_json::Value,
}

/// Parse a raw input string into source fields.
///
/// - Inputs starting with `http://` or `https://` are treated as URLs.
///   The domain is matched against known platforms (Instagram, Facebook, X/Twitter,
///   TikTok, GoFundMe) and social handles are extracted from the path.
///   Anything else becomes a `website` source.
/// - All other inputs are treated as search queries (`web_search`).
fn parse_source_input(input: &str) -> ParsedInput {
    let input = input.trim();

    // Not a URL → web_search
    if !input.starts_with("http://") && !input.starts_with("https://") {
        return ParsedInput {
            name: input.to_string(),
            source_type: "web_search".to_string(),
            url: None,
            handle: None,
            config: serde_json::json!({ "search_query": input, "max_results": 10 }),
        };
    }

    let parsed = match Url::parse(input) {
        Ok(u) => u,
        Err(_) => {
            return ParsedInput {
                name: input.to_string(),
                source_type: "website".to_string(),
                url: Some(input.to_string()),
                handle: None,
                config: serde_json::json!({}),
            };
        }
    };

    let host = parsed.host_str().unwrap_or("").to_lowercase();
    // Strip leading "www." for matching
    let domain = host.strip_prefix("www.").unwrap_or(&host);

    // Extract the first meaningful path segment (strip leading slashes and @)
    let path_segment = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .trim_start_matches('@')
        .to_string();

    match domain {
        "instagram.com" => {
            let handle = if path_segment.is_empty() { domain.to_string() } else { path_segment.clone() };
            ParsedInput {
                name: handle.clone(),
                source_type: "instagram".to_string(),
                url: Some(input.to_string()),
                handle: Some(handle),
                config: serde_json::json!({}),
            }
        }
        "facebook.com" | "fb.com" => {
            let handle = if path_segment.is_empty() { domain.to_string() } else { path_segment.clone() };
            ParsedInput {
                name: handle.clone(),
                source_type: "facebook".to_string(),
                url: Some(input.to_string()),
                handle: Some(handle),
                config: serde_json::json!({}),
            }
        }
        "x.com" | "twitter.com" => {
            let handle = if path_segment.is_empty() { domain.to_string() } else { path_segment.clone() };
            ParsedInput {
                name: handle.clone(),
                source_type: "x".to_string(),
                url: Some(input.to_string()),
                handle: Some(handle),
                config: serde_json::json!({}),
            }
        }
        "tiktok.com" => {
            let handle = if path_segment.is_empty() { domain.to_string() } else { path_segment.clone() };
            ParsedInput {
                name: handle.clone(),
                source_type: "tiktok".to_string(),
                url: Some(input.to_string()),
                handle: Some(handle),
                config: serde_json::json!({}),
            }
        }
        "gofundme.com" => {
            let name = if path_segment.is_empty() || path_segment == "f" {
                // Try to get the campaign slug from the path: /f/campaign-slug
                parsed.path().trim_start_matches('/').split('/').nth(1)
                    .unwrap_or("gofundme")
                    .replace('-', " ")
            } else {
                path_segment.replace('-', " ")
            };
            ParsedInput {
                name,
                source_type: "gofundme".to_string(),
                url: Some(input.to_string()),
                handle: None,
                config: serde_json::json!({}),
            }
        }
        "api.usaspending.gov" => {
            ParsedInput {
                name: format!("USAspending: {}", parsed.query().unwrap_or("all")),
                source_type: "usaspending".to_string(),
                url: Some(input.to_string()),
                handle: None,
                config: serde_json::json!({
                    "api_url": input,
                }),
            }
        }
        "echodata.epa.gov" => {
            ParsedInput {
                name: format!("EPA ECHO: {}", parsed.query().unwrap_or("all")),
                source_type: "epa_echo".to_string(),
                url: Some(input.to_string()),
                handle: None,
                config: serde_json::json!({
                    "api_url": input,
                }),
            }
        }
        _ => {
            // Generic website — use domain as name
            ParsedInput {
                name: domain.to_string(),
                source_type: "website".to_string(),
                url: Some(input.to_string()),
                handle: None,
                config: serde_json::json!({}),
            }
        }
    }
}

impl Source {
    /// Convenience method to get the effective cadence for this source.
    pub fn effective_cadence_hours(&self) -> i32 {
        compute_cadence(&self.source_type, self.consecutive_misses)
    }

    /// Create a source from a raw user input (URL or search query).
    ///
    /// Automatically detects source type, extracts name/handle, and creates
    /// appropriate child records (website_sources, social_sources).
    pub async fn create_from_input(input: &str, pool: &PgPool) -> Result<Self> {
        let parsed = parse_source_input(input);

        let source = Self::create(
            &parsed.name,
            &parsed.source_type,
            parsed.url.as_deref(),
            parsed.handle.as_deref(),
            None,
            parsed.config,
            pool,
        )
        .await?;

        // Create child records for typed sources
        match parsed.source_type.as_str() {
            "website" => {
                if let Some(url) = &parsed.url {
                    if let Ok(u) = Url::parse(url) {
                        if let Some(host) = u.host_str() {
                            let domain = host.strip_prefix("www.").unwrap_or(host);
                            let _ = WebsiteSource::create(source.id, domain, 2, pool).await;
                        }
                    }
                }
            }
            "instagram" | "facebook" | "x" | "tiktok" => {
                if let Some(handle) = &parsed.handle {
                    let _ = SocialSource::create(source.id, &parsed.source_type, handle, pool).await;
                }
            }
            _ => {}
        }

        Ok(source)
    }

    pub async fn create(
        name: &str,
        source_type: &str,
        url: Option<&str>,
        handle: Option<&str>,
        entity_id: Option<Uuid>,
        config: serde_json::Value,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO sources (name, source_type, url, handle, entity_id, config, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, TRUE)
            RETURNING *
            "#,
        )
        .bind(name)
        .bind(source_type)
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

    pub async fn find_pending_qualification(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM sources WHERE qualification_status = 'pending' ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find an existing website source by domain, or create a new one.
    /// Returns `(source, was_created)`.
    pub async fn find_or_create_website(
        name: &str,
        url: &str,
        discovered_from: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<(Self, bool)> {
        let parsed = Url::parse(url)?;
        let domain = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("URL has no host: {}", url))?
            .to_string();

        // Check if a website_source with this domain already exists
        let existing = sqlx::query_as::<_, (Uuid,)>(
            "SELECT source_id FROM website_sources WHERE domain = $1",
        )
        .bind(&domain)
        .fetch_optional(pool)
        .await?;

        if let Some((source_id,)) = existing {
            let source = Self::find_by_id(source_id, pool).await?;
            return Ok((source, false));
        }

        // Create new source
        let mut config = serde_json::json!({});
        if let Some(parent_id) = discovered_from {
            config["discovered_from"] = serde_json::json!(parent_id.to_string());
        }

        let source = Self::create(name, "website", Some(url), None, None, config, pool).await?;

        // Create website_source record
        WebsiteSource::create(source.id, &domain, 2, pool).await?;

        Ok((source, true))
    }

    /// Find an existing social source by (platform, handle), or create a new one.
    /// Returns `(source, was_created)`.
    pub async fn find_or_create_social(
        platform: &str,
        handle: &str,
        url: Option<&str>,
        entity_id: Uuid,
        pool: &PgPool,
    ) -> Result<(Self, bool)> {
        // Check if a social_source with this platform+handle already exists
        let existing = sqlx::query_as::<_, (Uuid,)>(
            "SELECT source_id FROM social_sources WHERE platform = $1 AND handle = $2",
        )
        .bind(platform)
        .bind(handle)
        .fetch_optional(pool)
        .await?;

        if let Some((source_id,)) = existing {
            let source = Self::find_by_id(source_id, pool).await?;
            return Ok((source, false));
        }

        // Create new source (inactive by default)
        let name = format!("{}@{}", handle, platform);
        let source = Self::create(
            &name,
            platform,
            url,
            Some(handle),
            Some(entity_id),
            serde_json::json!({}),
            pool,
        )
        .await?;

        // Create social_source record
        SocialSource::create(source.id, platform, handle, pool).await?;

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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WebsiteSource {
    pub id: Uuid,
    pub source_id: Uuid,
    pub domain: String,
    pub max_crawl_depth: i32,
    pub is_trusted: bool,
}

impl WebsiteSource {
    pub async fn create(
        source_id: Uuid,
        domain: &str,
        max_crawl_depth: i32,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO website_sources (source_id, domain, max_crawl_depth)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(source_id)
        .bind(domain)
        .bind(max_crawl_depth)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_source_id(source_id: Uuid, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM website_sources WHERE source_id = $1")
            .bind(source_id)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SocialSource {
    pub id: Uuid,
    pub source_id: Uuid,
    pub platform: String,
    pub handle: String,
}

impl SocialSource {
    pub async fn create(
        source_id: Uuid,
        platform: &str,
        handle: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO social_sources (source_id, platform, handle)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(source_id)
        .bind(platform)
        .bind(handle)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_cadence_base_cases() {
        assert_eq!(compute_cadence("website", 0), 168);
        assert_eq!(compute_cadence("web_search", 0), 24);
        assert_eq!(compute_cadence("search_query", 0), 24);
        assert_eq!(compute_cadence("instagram", 0), 12);
        assert_eq!(compute_cadence("facebook", 0), 12);
        assert_eq!(compute_cadence("x", 0), 12);
        assert_eq!(compute_cadence("tiktok", 0), 12);
        assert_eq!(compute_cadence("gofundme", 0), 12);
        assert_eq!(compute_cadence("usaspending", 0), 168);
        assert_eq!(compute_cadence("epa_echo", 0), 168);
    }

    #[test]
    fn test_compute_cadence_backoff() {
        // social: base=12, ceiling=72
        assert_eq!(compute_cadence("instagram", 1), 24);
        assert_eq!(compute_cadence("instagram", 2), 48);
        assert_eq!(compute_cadence("instagram", 3), 72); // hits ceiling

        // search: base=24, ceiling=72
        assert_eq!(compute_cadence("web_search", 1), 48);
        assert_eq!(compute_cadence("web_search", 2), 72); // hits ceiling

        // website: base=168, ceiling=360
        assert_eq!(compute_cadence("website", 1), 336);
        assert_eq!(compute_cadence("website", 2), 360); // hits ceiling

        // institutional: base=168, ceiling=720
        assert_eq!(compute_cadence("usaspending", 1), 336);
        assert_eq!(compute_cadence("usaspending", 2), 672);
        assert_eq!(compute_cadence("usaspending", 3), 720); // hits ceiling
    }

    #[test]
    fn test_compute_cadence_ceilings() {
        assert_eq!(compute_cadence("instagram", 10), 72);
        assert_eq!(compute_cadence("web_search", 10), 72);
        assert_eq!(compute_cadence("website", 10), 360);
        assert_eq!(compute_cadence("usaspending", 10), 720);
    }

    #[test]
    fn test_parse_source_input_website() {
        let p = parse_source_input("https://example.com/some/page");
        assert_eq!(p.source_type, "website");
        assert_eq!(p.name, "example.com");
        assert_eq!(p.url.as_deref(), Some("https://example.com/some/page"));
        assert!(p.handle.is_none());
    }

    #[test]
    fn test_parse_source_input_website_www() {
        let p = parse_source_input("https://www.example.com");
        assert_eq!(p.source_type, "website");
        assert_eq!(p.name, "example.com");
    }

    #[test]
    fn test_parse_source_input_instagram() {
        let p = parse_source_input("https://www.instagram.com/somecoffeeshop");
        assert_eq!(p.source_type, "instagram");
        assert_eq!(p.name, "somecoffeeshop");
        assert_eq!(p.handle.as_deref(), Some("somecoffeeshop"));
    }

    #[test]
    fn test_parse_source_input_x() {
        let p = parse_source_input("https://x.com/someuser");
        assert_eq!(p.source_type, "x");
        assert_eq!(p.name, "someuser");
        assert_eq!(p.handle.as_deref(), Some("someuser"));
    }

    #[test]
    fn test_parse_source_input_twitter() {
        let p = parse_source_input("https://twitter.com/someuser");
        assert_eq!(p.source_type, "x");
        assert_eq!(p.name, "someuser");
    }

    #[test]
    fn test_parse_source_input_facebook() {
        let p = parse_source_input("https://facebook.com/somepage");
        assert_eq!(p.source_type, "facebook");
        assert_eq!(p.name, "somepage");
    }

    #[test]
    fn test_parse_source_input_tiktok() {
        let p = parse_source_input("https://tiktok.com/@someuser");
        assert_eq!(p.source_type, "tiktok");
        assert_eq!(p.name, "someuser");
        assert_eq!(p.handle.as_deref(), Some("someuser"));
    }

    #[test]
    fn test_parse_source_input_gofundme() {
        let p = parse_source_input("https://www.gofundme.com/f/help-rebuild-community-center");
        assert_eq!(p.source_type, "gofundme");
        assert_eq!(p.name, "help rebuild community center");
    }

    #[test]
    fn test_parse_source_input_search_query() {
        let p = parse_source_input("third places community spaces Minneapolis");
        assert_eq!(p.source_type, "web_search");
        assert_eq!(p.name, "third places community spaces Minneapolis");
        assert!(p.url.is_none());
        assert_eq!(p.config["search_query"], "third places community spaces Minneapolis");
        assert_eq!(p.config["max_results"], 10);
    }

    #[test]
    fn test_parse_source_input_whitespace_trimmed() {
        let p = parse_source_input("  https://example.com  ");
        assert_eq!(p.source_type, "website");
        assert_eq!(p.name, "example.com");
    }

    #[test]
    fn test_parse_source_input_usaspending() {
        let p = parse_source_input("https://api.usaspending.gov/api/v2/search/spending_by_award/?recipient=GEO+Group");
        assert_eq!(p.source_type, "usaspending");
        assert!(p.name.starts_with("USAspending:"));
        assert!(p.url.is_some());
    }

    #[test]
    fn test_parse_source_input_epa_echo() {
        let p = parse_source_input("https://echodata.epa.gov/echo/dfr_rest_services.get_facility_info?p_name=Acme");
        assert_eq!(p.source_type, "epa_echo");
        assert!(p.name.starts_with("EPA ECHO:"));
        assert!(p.url.is_some());
    }
}
