use anyhow::Result;
use rootsignal_core::ServerDeps;

use crate::taxonomy::build_tag_instructions;
use super::types::ParsedQuery;

/// Parse a natural language query into structured filters + search text using AI.
pub async fn parse_natural_language_query(
    query: &str,
    deps: &ServerDeps,
) -> Result<ParsedQuery> {
    let pool = deps.pool();
    let tag_instructions = build_tag_instructions("listing", pool).await?;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let system_prompt = deps.prompts.nlq_prompt(&tag_instructions, &today);

    let model = &deps.file_config.models.nlq;

    let parsed: ParsedQuery = deps
        .ai
        .extract(model, &system_prompt, query)
        .await?;

    Ok(parsed)
}
