use anyhow::{bail, Result};
use rootsignal_core::ServerDeps;
use serde::Deserialize;
use uuid::Uuid;

use crate::entities::activities::{discover_social_for_entity, discover_social_from_url};
use crate::entities::Entity;
use crate::scraping::Source;

/// AI-extracted entity information from a source's scraped pages.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ExtractedEntity {
    /// The official name of the organization or entity that operates this source.
    pub name: String,
    /// "Organization", "GovernmentEntity", or "LocalBusiness".
    pub entity_type: String,
    /// A brief 1-3 sentence description of the entity.
    pub description: Option<String>,
}

/// Detect the entity behind a source using AI extraction, then find-or-create
/// the entity and link it to the source.
pub async fn detect_source_entity(source_id: Uuid, deps: &ServerDeps) -> Result<Entity> {
    let pool = deps.pool();
    let source = Source::find_by_id(source_id, pool).await?;

    if source.entity_id.is_some() {
        bail!("Source already has an entity linked");
    }

    tracing::info!(source_id = %source_id, name = %source.name, "Detecting entity for source");

    // Load scraped page content
    let snapshot_rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        r#"
        SELECT ps.url, ps.raw_content, ps.html
        FROM page_snapshots ps
        JOIN domain_snapshots ds ON ds.page_snapshot_id = ps.id
        WHERE ds.source_id = $1
        ORDER BY ps.crawled_at DESC
        LIMIT 5
        "#,
    )
    .bind(source_id)
    .fetch_all(pool)
    .await?;

    if snapshot_rows.is_empty() {
        bail!("No scraped pages available. Run a scrape first.");
    }

    // Build user prompt with page content
    let mut user_prompt = format!(
        "Source: {} (type: {})\nURL: {}\nHandle: {}\n\nScraped pages:\n\n",
        source.name,
        source.source_type(),
        source.url.as_deref().unwrap_or("—"),
        source.handle.as_deref().unwrap_or("—"),
    );

    for (i, (url, content, html)) in snapshot_rows.iter().enumerate() {
        let content = content.as_deref().or(html.as_deref()).unwrap_or("[empty]");
        let truncated = if content.len() > 8000 {
            &content[..8000]
        } else {
            content
        };
        user_prompt.push_str(&format!("--- Page {} ---\nURL: {}\n{}\n\n", i + 1, url, truncated));
    }

    let model = &deps.file_config.models.extraction;
    let system_prompt = deps.prompts.detect_entity_prompt();

    let extracted: ExtractedEntity = deps.ai.extract(model, system_prompt, &user_prompt).await?;

    if extracted.name.to_lowercase() == "unknown" || extracted.name.len() < 2 {
        bail!("Could not determine entity from scraped content");
    }

    tracing::info!(
        source_id = %source_id,
        entity_name = %extracted.name,
        entity_type = %extracted.entity_type,
        "Entity detected"
    );

    // Find or create the entity
    let entity = Entity::find_or_create(
        &extracted.name,
        &extracted.entity_type,
        extracted.description.as_deref(),
        source.url.as_deref(),
        pool,
    )
    .await?;

    // Link entity to source
    Source::set_entity_id(source_id, entity.id, pool).await?;

    tracing::info!(
        source_id = %source_id,
        entity_id = %entity.id,
        "Source linked to entity"
    );

    // Best-effort social discovery: try page snapshots first, fall back to website URL
    if let Err(e) = async {
        let social_sources = discover_social_for_entity(entity.id, pool).await?;
        if social_sources.is_empty() {
            if let Some(ref website) = entity.website {
                discover_social_from_url(website, entity.id, &deps.http_client, pool).await?;
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await
    {
        tracing::warn!(
            entity_id = %entity.id,
            error = %e,
            "Social discovery failed (non-fatal)"
        );
    }


    Ok(entity)
}
