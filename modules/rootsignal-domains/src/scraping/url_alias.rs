use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UrlAlias {
    pub id: Uuid,
    pub original_url: String,
    pub canonical_url: String,
    pub redirect_type: Option<String>,
    pub discovered_at: DateTime<Utc>,
}

impl UrlAlias {
    pub async fn create(
        original_url: &str,
        canonical_url: &str,
        redirect_type: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO url_aliases (original_url, canonical_url, redirect_type)
            VALUES ($1, $2, $3)
            ON CONFLICT (original_url)
            DO UPDATE SET canonical_url = EXCLUDED.canonical_url,
                         redirect_type = EXCLUDED.redirect_type
            RETURNING *
            "#,
        )
        .bind(original_url)
        .bind(canonical_url)
        .bind(redirect_type)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_canonical(original_url: &str, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM url_aliases WHERE original_url = $1",
        )
        .bind(original_url)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all_aliases(canonical_url: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM url_aliases WHERE canonical_url = $1 ORDER BY discovered_at ASC",
        )
        .bind(canonical_url)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}

/// Normalize a URL by lowercasing the scheme and host, removing default ports,
/// sorting query parameters, removing trailing slashes, and stripping fragments.
pub fn normalize_url(raw: &str) -> Result<String> {
    let mut parsed = Url::parse(raw)?;

    // Remove fragment
    parsed.set_fragment(None);

    // Remove default ports
    if parsed.port() == Some(80) && parsed.scheme() == "http"
        || parsed.port() == Some(443) && parsed.scheme() == "https"
    {
        let _ = parsed.set_port(None);
    }

    // Sort query parameters
    if let Some(query) = parsed.query() {
        if !query.is_empty() {
            let mut pairs: Vec<(String, String)> = parsed
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            pairs.sort();
            let sorted: Vec<String> = pairs
                .iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        k.clone()
                    } else {
                        format!("{k}={v}")
                    }
                })
                .collect();
            parsed.set_query(Some(&sorted.join("&")));
        }
    }

    let mut result = parsed.to_string();

    // Remove trailing slash (unless path is just "/")
    if result.ends_with('/') && parsed.path() != "/" {
        result.pop();
    }

    Ok(result)
}
