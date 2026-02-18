use ai_client::claude::Claude;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::EditionNode;

use crate::writer::GraphWriter;
use crate::GraphClient;

/// Generates weekly editions that summarize stories for a city.
pub struct EditionGenerator {
    client: GraphClient,
    writer: GraphWriter,
    anthropic_api_key: String,
}

impl EditionGenerator {
    pub fn new(client: GraphClient, anthropic_api_key: &str) -> Self {
        Self {
            writer: GraphWriter::new(client.clone()),
            client,
            anthropic_api_key: anthropic_api_key.to_string(),
        }
    }

    /// Generate an edition for a city covering a time period.
    pub async fn generate_edition(
        &self,
        city: &str,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<EditionNode, Box<dyn std::error::Error + Send + Sync>> {
        // Find stories active in this period
        let stories = self.writer.get_stories_in_period(&period_start, &period_end).await?;

        if stories.is_empty() {
            return Err("No stories found in period".into());
        }

        info!(city, stories = stories.len(), "Generating edition");

        // Group stories by category
        let mut by_category: HashMap<String, Vec<(Uuid, String, f64)>> = HashMap::new();
        for (id, headline, category, energy) in &stories {
            let cat = if category.is_empty() { "community".to_string() } else { category.clone() };
            by_category
                .entry(cat)
                .or_default()
                .push((*id, headline.clone(), *energy));
        }

        // Build editorial summary prompt
        let mut category_summaries = Vec::new();
        for (category, cat_stories) in &by_category {
            let headlines: Vec<String> = cat_stories
                .iter()
                .take(5)
                .map(|(_, h, _)| format!("  - {h}"))
                .collect();
            category_summaries.push(format!("**{}**:\n{}", category, headlines.join("\n")));
        }

        // Count new signals in period
        let new_signal_count = self.count_signals_in_period(&period_start, &period_end).await?;

        // Generate editorial summary via LLM
        let period_label = format_period(&period_start);
        let editorial_summary = self.generate_editorial_summary(
            city,
            &period_label,
            &category_summaries,
            stories.len(),
            new_signal_count,
        ).await?;

        // Create edition node
        let edition = EditionNode {
            id: Uuid::new_v4(),
            city: city.to_string(),
            period: period_label,
            period_start,
            period_end,
            generated_at: Utc::now(),
            story_count: stories.len() as u32,
            new_signal_count,
            editorial_summary,
        };

        self.writer.create_edition(&edition).await?;

        // Link top stories
        for (story_id, _, _, _) in stories.iter().take(10) {
            if let Err(e) = self.writer.link_edition_to_story(edition.id, *story_id).await {
                warn!(error = %e, "Failed to link story to edition");
            }
        }

        info!(
            edition_id = %edition.id,
            city,
            stories = edition.story_count,
            signals = edition.new_signal_count,
            "Edition generated"
        );

        Ok(edition)
    }

    async fn count_signals_in_period(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<u32, neo4rs::Error> {
        use neo4rs::query;
        use crate::writer::format_datetime_pub;

        let q = query(
            "MATCH (n)
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
               AND datetime(n.extracted_at) >= datetime($start)
               AND datetime(n.extracted_at) <= datetime($end)
             RETURN count(n) AS cnt"
        )
        .param("start", format_datetime_pub(start))
        .param("end", format_datetime_pub(end));

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u32);
        }
        Ok(0)
    }

    async fn generate_editorial_summary(
        &self,
        city: &str,
        period: &str,
        category_summaries: &[String],
        story_count: usize,
        signal_count: u32,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let prompt = format!(
            r#"Write a brief editorial summary for the {city} community newspaper edition for {period}.

This week we tracked {story_count} stories across {signal_count} civic signals.

Stories by category:
{categories}

Write 3-5 sentences that:
1. Open with the most important story or theme this week
2. Highlight what community members should pay attention to
3. Note any emerging patterns or trends
4. Close with a forward-looking note

Write in a warm, community-focused tone. Be specific, not generic."#,
            categories = category_summaries.join("\n\n"),
        );

        let claude = Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");
        let response = claude.chat_completion(
            "You are the editor of a community newspaper. Write concise, warm editorial summaries.",
            &prompt,
        ).await?;

        Ok(response.trim().to_string())
    }
}

/// Format a DateTime as an ISO week period string, e.g. "2026-W07"
fn format_period(dt: &DateTime<Utc>) -> String {
    use chrono::Datelike;
    format!("{}-W{:02}", dt.year(), dt.iso_week().week())
}
