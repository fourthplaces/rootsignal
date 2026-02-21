use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{RegionNode, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_graph::GraphWriter;

use rootsignal_archive::Archive;

use crate::scraper::WebSearcher;
use crate::sources;

/// Handles cold-start bootstrapping for a brand-new city.
/// When no CityNode exists, this generates seed search queries,
/// performs a news sweep, and creates initial Source nodes.
pub struct Bootstrapper<'a> {
    writer: &'a GraphWriter,
    _searcher: &'a dyn WebSearcher,
    archive: Option<Arc<Archive>>,
    anthropic_api_key: String,
    region: RegionNode,
}

impl<'a> Bootstrapper<'a> {
    pub fn new(
        writer: &'a GraphWriter,
        searcher: &'a dyn WebSearcher,
        anthropic_api_key: &str,
        region: RegionNode,
    ) -> Self {
        Self {
            writer,
            _searcher: searcher,
            archive: None,
            anthropic_api_key: anthropic_api_key.to_string(),
            region,
        }
    }

    pub fn with_archive(mut self, archive: Arc<Archive>) -> Self {
        self.archive = Some(archive);
        self
    }

    /// Run the cold start bootstrap. Returns number of sources discovered.
    pub async fn run(&self) -> Result<u32> {
        info!(
            city = self.region.name.as_str(),
            "Starting cold start bootstrap..."
        );

        // Step 1: Generate seed queries using Claude Haiku
        let queries = self.generate_seed_queries().await?;
        info!(count = queries.len(), "Generated seed queries");

        // Step 2: Create WebQuery source nodes for each query
        let mut sources_created = 0u32;
        let now = Utc::now();
        for (query, role) in &queries {
            let cv = query.clone();
            let ck = sources::make_canonical_key(&cv);
            let source = SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: None,
                discovery_method: DiscoveryMethod::ColdStart,
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
                source_role: role.clone(),
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

    /// Use Claude Haiku to generate seed web search queries for the city.
    /// Returns (query, role) pairs so tension vs response sources are labeled correctly.
    async fn generate_seed_queries(&self) -> Result<Vec<(String, SourceRole)>> {
        let city = &self.region.name;
        let claude =
            ai_client::claude::Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");

        // Tension queries — surface friction, complaints, unmet needs
        let tension_prompt = format!(
            r#"Generate 15 search queries that would surface community tensions, problems, and unmet needs in {city}. These should find:
- Housing pressures and instability
- Public safety and community trust
- Infrastructure and utilities access
- Government accountability and institutional failures
- Environmental hazards and ecological harm
- Climate impacts, natural disasters, extreme weather
- Education access and youth needs
- Economic hardship and cost of living
- Immigration and cultural displacement
- Healthcare access and mental health
- Industrial impacts on land and water
- Rural access gaps and isolation

Each query should be the kind of thing someone would type into Google to find real community friction — not resources or programs.

Return ONLY the queries, one per line. No numbering, no explanations."#
        );

        // Response queries — surface organizations and efforts addressing needs
        let response_prompt = format!(
            r#"Generate 10 search queries that would surface organizations and efforts actively helping people in {city}. These should find:
- Mutual aid networks and community support
- Legal aid and advocacy organizations
- Food assistance and community kitchens
- Housing assistance and shelter programs
- Community health and mental health services
- Environmental restoration and conservation groups
- Disaster preparedness and community resilience programs
- Volunteer networks and community organizing

Each query should find specific organizations doing real work — not event calendars, festivals, or generic community directories.

Return ONLY the queries, one per line. No numbering, no explanations."#
        );

        // Social queries — surface where people are actually talking
        let social_prompt = format!(
            r#"Generate 10 social media search terms and hashtags for finding people talking about community issues in {city}. These should find:
- Local hashtags people actually use (not branded campaigns)
- Mutual aid and community support conversations
- Neighborhood-level discussion and organizing
- People expressing needs or offering help
- Community responses to local problems

Include a mix of:
- Hashtags (e.g. #{city}MutualAid, #{city}Community)
- Search terms for GoFundMe, Instagram, X/Twitter, TikTok
- Neighborhood or region-specific terms people use locally

Return ONLY the terms, one per line. No numbering, no explanations."#
        );

        let (tension_resp, response_resp, social_resp) = tokio::join!(
            claude.complete(&tension_prompt),
            claude.complete(&response_prompt),
            claude.complete(&social_prompt),
        );

        let mut queries = Vec::new();

        let parse_lines = |text: &str| -> Vec<String> {
            text.lines()
                .map(|l| l.trim().trim_start_matches('#').to_string())
                .filter(|l| !l.is_empty() && l.len() > 3)
                .collect()
        };

        if let Ok(text) = tension_resp {
            for q in parse_lines(&text) {
                queries.push((q, SourceRole::Tension));
            }
        }

        if let Ok(text) = response_resp {
            for q in parse_lines(&text) {
                queries.push((q, SourceRole::Response));
            }
        }

        if let Ok(text) = social_resp {
            for q in parse_lines(&text) {
                queries.push((q, SourceRole::Mixed));
            }
        }

        Ok(queries)
    }

    /// Generate standard platform sources for the city (Reddit, GoFundMe, Eventbrite, VolunteerMatch, etc.)
    /// Eventbrite/VolunteerMatch/GoFundMe are query sources — they produce URLs, not content.
    /// Reddit subreddits are discovered via LLM rather than guessed from the city slug.
    async fn generate_platform_sources(&self) -> Vec<SourceNode> {
        let _slug = &self.region.name;
        let city_name = &self.region.name;
        let city_name_encoded = city_name.replace(' ', "+");
        let now = Utc::now();

        let make_url = |url: &str, role: SourceRole| {
            let ck = sources::make_canonical_key(url);
            let cv = rootsignal_common::canonical_value(url);
            SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: Some(url.to_string()),
                discovery_method: DiscoveryMethod::ColdStart,
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
            let ck = sources::make_canonical_key(query);
            SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: query.to_string(),
                url: None,
                discovery_method: DiscoveryMethod::ColdStart,
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
                for (name, feed_url) in outlets {
                    // Validate the feed URL is reachable by attempting a fetch
                    let reachable = if let Some(ref archive) = self.archive {
                        archive.fetch(&feed_url).await.is_ok()
                    } else {
                        crate::scraper::RssFetcher::new().fetch_items(&feed_url).await.is_ok()
                    };
                    if reachable {
                        let mut source = make_url(
                            &feed_url,
                            SourceRole::Mixed,
                        );
                        source.canonical_value = name.clone();
                        source.gap_context = Some(format!("Outlet: {name}"));
                        sources.push(source);
                    } else {
                        warn!(outlet = name.as_str(), feed_url = feed_url.as_str(), "RSS feed unreachable, skipping");
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
        let city = &self.region.name;

        let prompt = format!(
            r#"What are the active Reddit subreddits specifically for {city} and its immediate metro area?

Rules:
- ONLY include subreddits dedicated to {city} or its immediate suburbs/neighborhoods
- Do NOT include state-level subreddits (e.g. r/Minnesota, r/Texas)
- Do NOT include national/global topic subreddits (e.g. r/FuckCars, r/urbanplanning, r/housing)
- Do NOT include subreddits for cities more than 30 miles away
- Each subreddit must have {city} or a neighborhood/suburb name in its name or be the well-known main sub for the city

Return ONLY the subreddit names (without r/ prefix), one per line. No numbering, no explanations.
Maximum 5 subreddits."#
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
            .take(5)
            .collect();

        Ok(subreddits)
    }

    /// Ask Claude for local news outlets and their RSS feed URLs.
    /// Returns (outlet_name, feed_url) pairs.
    async fn discover_news_outlets(&self) -> Result<Vec<(String, String)>> {
        let city = &self.region.name;

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
    region: &RegionNode,
) -> Result<Vec<SourceNode>> {
    // Get existing tensions from the graph
    let tensions = writer.get_recent_tensions(10).await.unwrap_or_default();
    if tensions.is_empty() {
        info!("No tensions found for tension-seeded discovery");
        return Ok(Vec::new());
    }

    let city = &region.name;
    let mut all_sources = Vec::new();
    let now = Utc::now();

    for (title, what_would_help) in &tensions {
        let help_text = what_would_help.as_deref().unwrap_or(title);
        let query = format!("organizations helping with {} in {}", help_text, city);

        let cv = query.clone();
        let ck = sources::make_canonical_key(&cv);
        all_sources.push(SourceNode {
            id: Uuid::new_v4(),
            canonical_key: ck,
            canonical_value: cv,
            url: None,
            discovery_method: DiscoveryMethod::TensionSeed,
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
