use anyhow::Result;
use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{CityNode, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_graph::GraphWriter;

use crate::scraper::{RssFetcher, WebSearcher};
use crate::sources;

/// Handles cold-start bootstrapping for a brand-new city.
/// When no CityNode exists, this generates seed search queries,
/// performs a news sweep, and creates initial Source nodes.
pub struct Bootstrapper<'a> {
    writer: &'a GraphWriter,
    _searcher: &'a dyn WebSearcher,
    anthropic_api_key: String,
    city_node: CityNode,
}

impl<'a> Bootstrapper<'a> {
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
        info!(
            city = self.city_node.name.as_str(),
            "Starting cold start bootstrap..."
        );

        // Step 1: Generate seed queries using Claude Haiku
        let queries = self.generate_seed_queries().await?;
        info!(count = queries.len(), "Generated seed queries");

        // Step 2: Create WebQuery source nodes for each query
        let mut sources_created = 0u32;
        let now = Utc::now();
        for query in &queries {
            let cv = query.clone();
            let ck = sources::make_canonical_key(&self.city_node.slug, &cv);
            let source = SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: None,
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
                quality_penalty: 1.0,
                source_role: SourceRole::default(),
                scrape_count: 0,
            };
            match self.writer.upsert_source(&source).await {
                Ok(_) => sources_created += 1,
                Err(e) => warn!(query = query.as_str(), error = %e, "Failed to create seed source"),
            }
        }

        // Step 3: Also create standard platform sources (including LLM-discovered subreddits)
        let platform_sources = self.generate_platform_sources().await;
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

    /// Use Claude Haiku to generate 20-30 seed web search queries for the city.
    async fn generate_seed_queries(&self) -> Result<Vec<String>> {
        let city = &self.city_node.name;

        let prompt = format!(
            r#"Generate 25 search queries to discover community life in {city}. Focus on:
- Community volunteer opportunities
- Food banks, food shelves, mutual aid
- Local government meetings, public hearings
- Housing advocacy, tenant rights
- Immigration support, sanctuary resources
- Youth and senior services
- Environmental cleanup, community gardens
- Community events, festivals
- Local news about tensions or community issues
- Donation drives, crowdfunding, solidarity funds, and community fundraisers
- Repair cafes, tool libraries
- School board and education policy

Return ONLY the queries, one per line. No numbering, no explanations."#
        );

        let claude =
            ai_client::claude::Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");

        let response = claude.complete(&prompt).await?;

        let queries: Vec<String> = response
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l.len() > 5)
            .collect();

        Ok(queries)
    }

    /// Generate standard platform sources for the city (Reddit, GoFundMe, Eventbrite, VolunteerMatch, etc.)
    /// Eventbrite/VolunteerMatch/GoFundMe are query sources — they produce URLs, not content.
    /// Reddit subreddits are discovered via LLM rather than guessed from the city slug.
    async fn generate_platform_sources(&self) -> Vec<SourceNode> {
        let slug = &self.city_node.slug;
        let city_name = &self.city_node.name;
        let city_name_encoded = city_name.replace(' ', "+");
        let now = Utc::now();

        let make_url = |url: &str, role: SourceRole| {
            let ck = sources::make_canonical_key(slug, url);
            let cv = rootsignal_common::canonical_value(url);
            SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: Some(url.to_string()),
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
                quality_penalty: 1.0,
                source_role: role,
                scrape_count: 0,
            }
        };

        let make_query = |query: &str, role: SourceRole| {
            let ck = sources::make_canonical_key(slug, query);
            SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: query.to_string(),
                url: None,
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
                quality_penalty: 1.0,
                source_role: role,
                scrape_count: 0,
            }
        };

        let mut sources = vec![
            make_url(
                &format!(
                    "https://www.eventbrite.com/d/united-states--{}/community/",
                    city_name_encoded
                ),
                SourceRole::Response,
            ),
            make_url(
                &format!(
                    "https://www.eventbrite.com/d/united-states--{}/volunteer/",
                    city_name_encoded
                ),
                SourceRole::Response,
            ),
            make_url(
                &format!(
                    "https://www.volunteermatch.org/search?l={}&k=&v=true",
                    city_name_encoded
                ),
                SourceRole::Response,
            ),
            // Site-scoped search: Serper will query `site:gofundme.com/f/ {city} {topic}`
            make_query(
                &format!("site:gofundme.com/f/ {}", city_name),
                SourceRole::Response,
            ),
        ];

        // Reddit — discover relevant subreddits via LLM
        match self.discover_subreddits().await {
            Ok(subreddits) => {
                info!(
                    count = subreddits.len(),
                    "Discovered subreddits for {}", city_name
                );
                for sub in subreddits {
                    sources.push(make_url(
                        &format!("https://www.reddit.com/r/{}/", sub),
                        SourceRole::Mixed,
                    ));
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to discover subreddits, skipping Reddit sources");
            }
        }

        // RSS — discover local news outlet feeds via LLM
        match self.discover_news_outlets().await {
            Ok(outlets) => {
                info!(
                    count = outlets.len(),
                    "Discovered news outlets for {}", city_name
                );
                let fetcher = RssFetcher::new();
                for (name, feed_url) in outlets {
                    // Validate the feed URL is reachable by attempting a fetch
                    match fetcher.fetch_items(&feed_url).await {
                        Ok(_) => {
                            let mut source = make_url(
                                &feed_url,
                                SourceRole::Mixed,
                            );
                            source.canonical_value = name.clone();
                            source.gap_context = Some(format!("Outlet: {name}"));
                            sources.push(source);
                        }
                        Err(e) => {
                            warn!(outlet = name.as_str(), feed_url = feed_url.as_str(), error = %e, "RSS feed unreachable, skipping");
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to discover news outlets, skipping RSS sources");
            }
        }

        sources
    }

    /// Ask Claude for relevant subreddits for this city.
    async fn discover_subreddits(&self) -> Result<Vec<String>> {
        let city = &self.city_node.name;

        let prompt = format!(
            r#"What are the active Reddit subreddits relevant to community life in {city}?

Include:
- The main city subreddit
- Neighborhood or metro area subreddits
- Local housing, community, or environment-focused subreddits

Only include subreddits that actually exist and are active.
Return ONLY the subreddit names (without r/ prefix), one per line. No numbering, no explanations.
Maximum 8 subreddits."#
        );

        let claude =
            ai_client::claude::Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");

        let response = claude.complete(&prompt).await?;

        let subreddits: Vec<String> = response
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches("r/")
                    .trim_start_matches("/r/")
                    .to_string()
            })
            .filter(|l| !l.is_empty() && !l.contains(' ') && l.len() >= 2)
            .take(8)
            .collect();

        Ok(subreddits)
    }

    /// Ask Claude for local news outlets and their RSS feed URLs.
    /// Returns (outlet_name, feed_url) pairs.
    async fn discover_news_outlets(&self) -> Result<Vec<(String, String)>> {
        let city = &self.city_node.name;

        let prompt = format!(
            r#"What are the local news outlets for {city}? Include newspapers, alt-weeklies, TV station news sites, and hyperlocal blogs.

Do NOT include national wire services (AP, Reuters, NPR national, CNN, Fox News).
Only include outlets that primarily cover {city} and its surrounding area.

For each outlet, provide the name and RSS/Atom feed URL if you know it.
Return as JSON array: [{{"name": "Outlet Name", "feed_url": "https://..."}}]
If you don't know the feed URL, use the homepage URL and append "/feed" as a guess.
Maximum 8 outlets. Return ONLY the JSON array, no explanation."#
        );

        let claude =
            ai_client::claude::Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");

        let response = claude.complete(&prompt).await?;

        // Parse JSON response — strip markdown code fences if present
        let json_str = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let outlets: Vec<NewsOutlet> = serde_json::from_str(json_str)
            .unwrap_or_else(|e| {
                warn!(error = %e, "Failed to parse news outlet JSON, trying line-by-line fallback");
                Vec::new()
            });

        Ok(outlets
            .into_iter()
            .filter(|o| !o.name.is_empty() && !o.feed_url.is_empty())
            .map(|o| (o.name, o.feed_url))
            .take(8)
            .collect())
    }
}

#[derive(serde::Deserialize)]
struct NewsOutlet {
    name: String,
    feed_url: String,
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
        let query = format!("organizations helping with {} in {}", help_text, city);

        let cv = query.clone();
        let ck = sources::make_canonical_key(&city_node.slug, &cv);
        all_sources.push(SourceNode {
            id: Uuid::new_v4(),
            canonical_key: ck,
            canonical_value: cv,
            url: None,
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
            quality_penalty: 1.0,
            source_role: SourceRole::Response,
            scrape_count: 0,
        });
    }

    info!(
        queries = all_sources.len(),
        "Generated tension-seeded queries"
    );
    Ok(all_sources)
}
