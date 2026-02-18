use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{DiscoveryMethod, SourceNode, SourceType};
use rootsignal_graph::GraphWriter;

use crate::sources;

/// Stats from a discovery run.
#[derive(Debug, Default)]
pub struct DiscoveryStats {
    pub actor_sources: u32,
    pub link_sources: u32,
    pub gap_sources: u32,
    pub duplicates_skipped: u32,
}

impl std::fmt::Display for DiscoveryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Discovery: actors={}, links={}, gaps={}, skipped={}",
            self.actor_sources, self.link_sources, self.gap_sources, self.duplicates_skipped
        )
    }
}

/// Discovers new sources from existing graph data.
pub struct SourceDiscoverer<'a> {
    writer: &'a GraphWriter,
    city_slug: String,
}

impl<'a> SourceDiscoverer<'a> {
    pub fn new(writer: &'a GraphWriter, city_slug: &str) -> Self {
        Self {
            writer,
            city_slug: city_slug.to_string(),
        }
    }

    /// Run all discovery triggers. Returns stats on what was found.
    pub async fn run(&self) -> DiscoveryStats {
        let mut stats = DiscoveryStats::default();

        // 1. Actor-mentioned sources — actors with domains/URLs that aren't tracked
        self.discover_from_actors(&mut stats).await;

        // 2. Coverage gap analysis — identify under-covered areas
        self.discover_from_gaps(&mut stats).await;

        if stats.actor_sources + stats.link_sources + stats.gap_sources > 0 {
            info!("{stats}");
        }

        stats
    }

    /// Find actors with domains/URLs that aren't already tracked as sources.
    async fn discover_from_actors(&self, stats: &mut DiscoveryStats) {
        let actors = match self.writer.get_actors_with_domains(&self.city_slug).await {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "Failed to get actors for discovery");
                return;
            }
        };

        let existing = match self.writer.get_active_sources(&self.city_slug).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to get existing sources for dedup");
                return;
            }
        };
        let existing_urls: std::collections::HashSet<String> = existing.iter()
            .filter_map(|s| s.url.as_ref().cloned())
            .collect();
        let existing_keys: std::collections::HashSet<String> = existing.iter()
            .map(|s| s.canonical_key.clone())
            .collect();

        let now = Utc::now();
        for (actor_name, domains, social_urls) in &actors {
            // Check each domain as a potential web source
            for domain in domains {
                let url = if domain.starts_with("http") {
                    domain.clone()
                } else {
                    format!("https://{domain}")
                };

                if existing_urls.contains(&url) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let cv = sources::canonical_value_from_url(SourceType::Web, &url);
                let ck = sources::make_canonical_key(&self.city_slug, SourceType::Web, &cv);
                if existing_keys.contains(&ck) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let source = SourceNode {
                    id: Uuid::new_v4(),
                    canonical_key: ck,
                    canonical_value: cv,
                    url: Some(url.clone()),
                    source_type: SourceType::Web,
                    discovery_method: DiscoveryMethod::SignalReference,
                    city: self.city_slug.clone(),
                    created_at: now,
                    last_scraped: None,
                    last_produced_signal: None,
                    signals_produced: 0,
                    signals_corroborated: 0,
                    consecutive_empty_runs: 0,
                    active: true,
                    gap_context: Some(format!("Actor: {actor_name}")),
                    weight: 0.3,
                    cadence_hours: None,
                    avg_signals_per_scrape: 0.0,
                    total_cost_cents: 0,
                    last_cost_cents: 0,
                    taxonomy_stats: None,
                    quality_penalty: 1.0,
                };

                match self.writer.upsert_source(&source).await {
                    Ok(_) => {
                        stats.actor_sources += 1;
                        info!(actor = actor_name.as_str(), url, "Discovered source from actor domain");
                    }
                    Err(e) => warn!(url, error = %e, "Failed to create actor-derived source"),
                }
            }

            // Check social URLs as potential sources
            for social_url in social_urls {
                if existing_urls.contains(social_url) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let source_type = SourceType::from_url(social_url);
                let cv = sources::canonical_value_from_url(source_type, social_url);
                let ck = sources::make_canonical_key(&self.city_slug, source_type, &cv);
                if existing_keys.contains(&ck) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let source = SourceNode {
                    id: Uuid::new_v4(),
                    canonical_key: ck,
                    canonical_value: cv,
                    url: Some(social_url.clone()),
                    source_type,
                    discovery_method: DiscoveryMethod::SignalReference,
                    city: self.city_slug.clone(),
                    created_at: now,
                    last_scraped: None,
                    last_produced_signal: None,
                    signals_produced: 0,
                    signals_corroborated: 0,
                    consecutive_empty_runs: 0,
                    active: true,
                    gap_context: Some(format!("Actor: {actor_name}")),
                    weight: 0.3,
                    cadence_hours: None,
                    avg_signals_per_scrape: 0.0,
                    total_cost_cents: 0,
                    last_cost_cents: 0,
                    taxonomy_stats: None,
                    quality_penalty: 1.0,
                };

                match self.writer.upsert_source(&source).await {
                    Ok(_) => {
                        stats.actor_sources += 1;
                        info!(actor = actor_name.as_str(), url = social_url.as_str(), "Discovered source from actor social");
                    }
                    Err(e) => warn!(url = social_url.as_str(), error = %e, "Failed to create actor social source"),
                }
            }
        }
    }

    /// Coverage gap analysis — generate targeted queries for under-covered tensions.
    async fn discover_from_gaps(&self, stats: &mut DiscoveryStats) {
        // Get tensions and existing source types to find gaps
        let tensions = match self.writer.get_recent_tensions(20).await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to get tensions for gap analysis");
                return;
            }
        };

        if tensions.is_empty() {
            return;
        }

        let existing = match self.writer.get_active_sources(&self.city_slug).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to get sources for gap analysis");
                return;
            }
        };

        let existing_queries: std::collections::HashSet<String> = existing.iter()
            .filter(|s| s.source_type == SourceType::TavilyQuery)
            .map(|s| s.canonical_value.to_lowercase())
            .collect();

        let now = Utc::now();
        let mut gap_count = 0u32;
        const MAX_GAP_QUERIES: u32 = 5;

        for (title, what_would_help) in &tensions {
            if gap_count >= MAX_GAP_QUERIES {
                break;
            }

            let help_text = what_would_help.as_deref().unwrap_or(title);
            let query = format!("{} resources services {}", help_text, self.city_slug);
            let query_lower = query.to_lowercase();

            // Skip if we already have a similar query
            if existing_queries.iter().any(|q| {
                q.contains(&query_lower) || query_lower.contains(q.as_str())
            }) {
                stats.duplicates_skipped += 1;
                continue;
            }

            let cv = query.clone();
            let ck = sources::make_canonical_key(&self.city_slug, SourceType::TavilyQuery, &cv);

            let source = SourceNode {
                id: Uuid::new_v4(),
                canonical_key: ck,
                canonical_value: cv,
                url: None,
                source_type: SourceType::TavilyQuery,
                discovery_method: DiscoveryMethod::GapAnalysis,
                city: self.city_slug.clone(),
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: Some(format!("Tension: {title}")),
                weight: 0.3,
                cadence_hours: None,
                avg_signals_per_scrape: 0.0,
                total_cost_cents: 0,
                last_cost_cents: 0,
                taxonomy_stats: None,
                quality_penalty: 1.0,
            };

            match self.writer.upsert_source(&source).await {
                Ok(_) => {
                    gap_count += 1;
                    stats.gap_sources += 1;
                    info!(tension = title.as_str(), query = source.canonical_value.as_str(), "Created gap analysis query");
                }
                Err(e) => warn!(error = %e, "Failed to create gap analysis source"),
            }
        }
    }
}

