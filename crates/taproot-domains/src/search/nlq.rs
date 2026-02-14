use anyhow::Result;
use taproot_core::ServerDeps;

use crate::entities::build_tag_instructions;
use super::types::ParsedQuery;

/// System prompt preamble for NLQ parsing.
const NLQ_SYSTEM_PREAMBLE: &str = r#"You are a query parser for Taproot, a community signal database for the Twin Cities (Minneapolis-St. Paul, Minnesota).

Given a natural language query, extract:
1. **search_text**: Free-text search terms (for semantic + full-text search). Remove taxonomy terms that are captured in filters.
2. **filters**: Taxonomy filter values. Only use exact values from the allowed taxonomy below.
3. **temporal**: Time-related intent (dates, day of week).
4. **intent**: Classify the query:
   - "in_scope": The query is about community services, events, resources, or opportunities in the Twin Cities.
   - "out_of_scope": The query is not related to community signals (e.g., "what's the weather?").
   - "needs_clarification": The query is too vague to be useful.
   - "knowledge_question": The query asks for general knowledge, not a search (e.g., "what is mutual aid?").
5. **reasoning**: Brief explanation of your classification and extraction decisions.

## Date Context
Today's date is: {today}

For temporal references:
- "today" = {today}
- "this weekend" = next Saturday and Sunday
- "tomorrow" = the day after {today}
- Use ISO 8601 dates (YYYY-MM-DD) for happening_on.
- Use "YYYY-MM-DD/YYYY-MM-DD" for happening_between ranges.
- Use iCal day codes for day_of_week: "MO", "TU", "WE", "TH", "FR", "SA", "SU"

## Available Taxonomy
"#;

/// Parse a natural language query into structured filters + search text using AI.
pub async fn parse_natural_language_query(
    query: &str,
    deps: &ServerDeps,
) -> Result<ParsedQuery> {
    let pool = deps.pool();
    let tag_instructions = build_tag_instructions("listing", pool).await?;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let system_prompt = format!(
        "{}\n\n{}",
        NLQ_SYSTEM_PREAMBLE.replace("{today}", &today),
        tag_instructions,
    );

    let parsed: ParsedQuery = deps
        .ai
        .extract("gpt-4o-mini", &system_prompt, query)
        .await?;

    Ok(parsed)
}
