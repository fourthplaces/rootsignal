use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::tag::Tag;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TagKindConfig {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub allowed_resource_types: Vec<String>,
    pub required: bool,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
}

impl TagKindConfig {
    pub async fn find_all(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM tag_kinds ORDER BY slug")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_for_resource_type(resource_type: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM tag_kinds WHERE $1 = ANY(allowed_resource_types) ORDER BY slug",
        )
        .bind(resource_type)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn tag_count_for_slug(slug: &str, pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM tags WHERE kind = $1")
            .bind(slug)
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }
}

/// Build AI prompt taxonomy instructions from the database.
///
/// Queries tag_kinds for the given resource type, loads all tag values per kind,
/// and formats them as structured instructions for the extraction prompt.
pub async fn build_tag_instructions(resource_type: &str, pool: &PgPool) -> Result<String> {
    let kinds = TagKindConfig::find_for_resource_type(resource_type, pool).await?;
    let mut lines = Vec::new();

    for kind in &kinds {
        let tags = Tag::find_by_kind(&kind.slug, pool).await?;
        if tags.is_empty() {
            continue;
        }

        let values: Vec<&str> = tags.iter().map(|t| t.value.as_str()).collect();
        let values_str = values
            .iter()
            .map(|v| format!("\"{}\"", v))
            .collect::<Vec<_>>()
            .join(", ");

        let required_label = if kind.required { " (required)" } else { "" };
        let desc = kind.description.as_deref().unwrap_or(&kind.display_name);

        lines.push(format!(
            "- **{}**{}: {} — Pick from: {}",
            kind.slug, required_label, desc, values_str
        ));
    }

    Ok(lines.join("\n"))
}
