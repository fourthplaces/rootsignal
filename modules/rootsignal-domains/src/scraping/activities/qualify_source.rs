use anyhow::Result;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::scraping::Source;

/// AI verdict on whether a source is worth scraping.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SourceQualification {
    /// 0-100 score for how likely this source is to produce community listings.
    pub score: i32,
    /// "green", "yellow", or "red".
    pub verdict: String,
    /// One-paragraph explanation of the decision.
    pub reasoning: String,
    /// What types of listings this source would likely yield (e.g. "events", "services", "programs").
    pub expected_listing_types: Vec<String>,
}

const QUALIFY_SYSTEM_PROMPT: &str = r#"You are evaluating whether a data source is likely to produce useful community listings for a local resource directory.

Community listings include: events, services, programs, classes, support groups, volunteer opportunities, food distribution, shelters, legal aid, health clinics, faith gatherings, recreation, civic meetings, etc.

You will receive sample pages scraped from a source. Analyze them as a batch and determine:
1. Does this source contain or link to community-relevant content?
2. How rich/structured is the information (names, dates, locations, descriptions)?
3. Is this a directory, calendar, or organization page vs. unrelated content (news, commerce, personal blog)?

Score 0-100:
- 80-100 (green): Strong source — directory, calendar, or org with structured community data
- 50-79 (yellow): Possible source — some relevant content but mixed or sparse
- 0-49 (red): Poor source — unrelated, paywalled, or too thin to extract listings from"#;

/// Run a sample scrape on a source, then ask AI to evaluate the batch.
pub async fn qualify_source(source_id: Uuid, deps: &ServerDeps) -> Result<SourceQualification> {
    let pool = deps.pool();
    let source = Source::find_by_id(source_id, pool).await?;

    tracing::info!(source_id = %source_id, name = %source.name, "Qualifying source");

    // Override limit to a small sample
    let mut sample_config = source.config.clone();
    sample_config["limit"] = serde_json::json!(5);

    // Run a sample scrape to populate snapshots for evaluation
    let _ = super::scrape_source::scrape_source(source_id, deps).await?;

    // Load the sample snapshots' content via domain_snapshots join
    let snapshot_rows = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>)>(
        r#"
        SELECT ps.id, ps.url, ps.markdown, ps.html
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
        // No pages fetched — auto-red
        let result = SourceQualification {
            score: 0,
            verdict: "red".to_string(),
            reasoning: "No pages could be fetched from this source.".to_string(),
            expected_listing_types: vec![],
        };
        update_source_qualification(source_id, &result, pool).await?;
        return Ok(result);
    }

    // Build the user prompt with all sample pages
    let mut user_prompt = format!(
        "Source: {} (type: {})\n\nSample pages ({} fetched, showing up to 5):\n\n",
        source.name,
        source.source_type,
        snapshot_rows.len(),
    );

    for (i, (_, url, markdown, html)) in snapshot_rows.iter().enumerate() {
        let content = markdown.as_deref().or(html.as_deref()).unwrap_or("[empty]");
        // Truncate each page to keep within token budget
        let truncated = if content.len() > 4000 {
            &content[..4000]
        } else {
            content
        };
        user_prompt.push_str(&format!(
            "--- Page {} ---\nURL: {}\n{}\n\n",
            i + 1,
            url,
            truncated,
        ));
    }

    let model = &deps.file_config.models.extraction;

    let result: SourceQualification = deps
        .ai
        .extract(model, QUALIFY_SYSTEM_PROMPT, &user_prompt)
        .await?;

    update_source_qualification(source_id, &result, pool).await?;

    // Auto-activate green sources, deactivate red ones
    match result.verdict.as_str() {
        "green" => {
            sqlx::query("UPDATE sources SET is_active = TRUE WHERE id = $1")
                .bind(source_id)
                .execute(pool)
                .await?;
        }
        "red" => {
            sqlx::query("UPDATE sources SET is_active = FALSE WHERE id = $1")
                .bind(source_id)
                .execute(pool)
                .await?;
        }
        _ => {} // yellow: leave as-is for human review
    }

    tracing::info!(
        source_id = %source_id,
        score = result.score,
        verdict = %result.verdict,
        "Source qualification complete"
    );

    Ok(result)
}

async fn update_source_qualification(
    source_id: Uuid,
    result: &SourceQualification,
    pool: &sqlx::PgPool,
) -> Result<()> {
    sqlx::query(
        "UPDATE sources SET qualification_status = $2, qualification_summary = $3, qualification_score = $4 WHERE id = $1",
    )
    .bind(source_id)
    .bind(&result.verdict)
    .bind(&result.reasoning)
    .bind(result.score)
    .execute(pool)
    .await?;
    Ok(())
}
