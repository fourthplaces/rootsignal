use anyhow::Result;
use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{CityNode, DiscoveryMethod, SourceNode, SourceType};
use rootsignal_graph::GraphWriter;

use crate::scraper::WebSearcher;
use crate::sources;

/// Handles cold-start bootstrapping for a brand-new city.
/// When no CityNode exists, this generates seed search queries,
/// performs a news sweep, and creates initial Source nodes.
pub struct ColdStartBootstrapper<'a> {
    writer: &'a GraphWriter,
    _searcher: &'a dyn WebSearcher,
    anthropic_api_key: String,
    city_node: CityNode,
}

impl<'a> ColdStartBootstrapper<'a> {
    pub fn new(
        writer: &'a GraphWriter,
        searcher: &'a dyn WebSearcher,
        anthropic_api_key: &str,
        city_node: CityNode,
    ) -> Self {
        Self {
            writer,
            _searcher: searcher,
            anthropic_api_key: anthropic_api_key.to_string(),
            city_node,
        }
    }

    /// Run the cold start bootstrap. Returns number of sources discovered.
    pub async fn run(&self) -> Result<u32> {
        info!(city = self.city_node.name.as_str(), "Starting cold start bootstrap...");

        // Step 1: Generate seed queries using Claude Haiku
        let queries = self.generate_seed_queries().await?;
        info!(count = queries.len(), "Generated seed queries");

        // Step 2: Create TavilyQuery source nodes for each query
        let mut sources_created = 0u32;
        let now = Utc::now();
        for query in &queries {
            let cv = query.clone();
            let ck = sources::make_canonical_key(&self.city_node.slug, SourceType::TavilyQuery, &cv);
            let source = SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: None,
                source_type: SourceType::TavilyQuery,
                discovery_method: DiscoveryMethod::ColdStart,
                city: self.city_node.slug.clone(),
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: None,
                weight: 0.5,
                cadence_hours: None,
                avg_signals_per_scrape: 0.0,
                total_cost_cents: 0,
                last_cost_cents: 0,
                taxonomy_stats: None,
                quality_penalty: 1.0,
            };
            match self.writer.upsert_source(&source).await {
                Ok(_) => sources_created += 1,
                Err(e) => warn!(query = query.as_str(), error = %e, "Failed to create seed source"),
            }
        }

        // Step 3: Also create standard platform sources
        let platform_sources = self.generate_platform_sources();
        for source in &platform_sources {
            match self.writer.upsert_source(source).await {
                Ok(_) => sources_created += 1,
                Err(e) => {
                    let label = source.url.as_deref().unwrap_or(&source.canonical_value);
                    warn!(source = label, error = %e, "Failed to create platform source");
                }
            }
        }

        info!(sources_created, "Cold start bootstrap complete");
        Ok(sources_created)
    }

    /// Use Claude Haiku to generate 20-30 seed Tavily search queries for the city.
    async fn generate_seed_queries(&self) -> Result<Vec<String>> {
        let city = &self.city_node.name;

        let prompt = format!(
            r#"Generate 25 search queries to discover civic life in {city}. Focus on:
- Community volunteer opportunities
- Food banks, food shelves, mutual aid
- Local government meetings, public hearings
- Housing advocacy, tenant rights
- Immigration support, sanctuary resources
- Youth and senior services
- Environmental cleanup, community gardens
- Community events, festivals
- Local news about civic tensions or community issues
- GoFundMe and community fundraisers
- Repair cafes, tool libraries
- School board and education policy

Return ONLY the queries, one per line. No numbering, no explanations."#
        );

        let claude = ai_client::claude::Claude::new(
            &self.anthropic_api_key,
            "claude-haiku-4-5-20251001",
        );

        let response = claude.complete(&prompt).await?;

        let queries: Vec<String> = response
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l.len() > 5)
            .collect();

        Ok(queries)
    }

    /// Generate standard platform sources for the city (Reddit, GoFundMe, Eventbrite, etc.)
    fn generate_platform_sources(&self) -> Vec<SourceNode> {
        let slug = &self.city_node.slug;
        let city_name = &self.city_node.name;
        let city_name_encoded = city_name.replace(' ', "+");
        let now = Utc::now();

        let make = |source_type: SourceType, url: &str| {
            let cv = sources::canonical_value_from_url(source_type, url);
            let ck = sources::make_canonical_key(slug, source_type, &cv);
            SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: Some(url.to_string()),
                source_type,
                discovery_method: DiscoveryMethod::ColdStart,
                city: slug.clone(),
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: None,
                weight: 0.5,
                cadence_hours: None,
                avg_signals_per_scrape: 0.0,
                total_cost_cents: 0,
                last_cost_cents: 0,
                taxonomy_stats: None,
                quality_penalty: 1.0,
            }
        };

        let mut sources = vec![
            make(SourceType::Web, &format!(
                "https://www.eventbrite.com/d/united-states--{}/community/",
                city_name_encoded
            )),
            make(SourceType::Web, &format!(
                "https://www.eventbrite.com/d/united-states--{}/volunteer/",
                city_name_encoded
            )),
            make(SourceType::Web, &format!(
                "https://www.volunteermatch.org/search?l={}&k=&v=true",
                city_name_encoded
            )),
            make(SourceType::Web, &format!(
                "https://www.gofundme.com/discover/search?q={}&location={}",
                slug, city_name_encoded
            )),
        ];

        // Reddit — try to add city subreddit
        let slug_lower = slug.to_lowercase();
        sources.push(make(
            SourceType::Reddit,
            &format!("https://www.reddit.com/r/{}", slug_lower),
        ));

        sources
    }
}

/// Generate tension-seeded follow-up queries from existing tensions.
/// For each tension, creates targeted search queries to find organizations helping.
pub async fn tension_seed_queries(
    writer: &GraphWriter,
    city_node: &CityNode,
) -> Result<Vec<SourceNode>> {
    // Get existing tensions from the graph
    let tensions = writer.get_recent_tensions(10).await.unwrap_or_default();
    if tensions.is_empty() {
        info!("No tensions found for tension-seeded discovery");
        return Ok(Vec::new());
    }

    let city = &city_node.name;
    let mut all_sources = Vec::new();
    let now = Utc::now();

    for (title, what_would_help) in &tensions {
        let help_text = what_would_help.as_deref().unwrap_or(title);
        let query = format!(
            "organizations helping with {} in {}",
            help_text, city
        );

        let cv = query.clone();
        let ck = sources::make_canonical_key(&city_node.slug, SourceType::TavilyQuery, &cv);
        all_sources.push(SourceNode {
            id: Uuid::new_v4(),
            canonical_key: ck,
            canonical_value: cv,
            url: None,
            source_type: SourceType::TavilyQuery,
            discovery_method: DiscoveryMethod::TensionSeed,
            city: city_node.slug.clone(),
            created_at: now,
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: Some(format!("Tension: {title}")),
            weight: 0.5,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            total_cost_cents: 0,
            last_cost_cents: 0,
            taxonomy_stats: None,
            quality_penalty: 1.0,
        });
    }

    info!(queries = all_sources.len(), "Generated tension-seeded queries");
    Ok(all_sources)
}
