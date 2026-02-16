use anyhow::{Context, Result};
use chrono::Utc;
use futures::stream::{self, StreamExt};
use tracing::{error, info, warn};
use uuid::Uuid;

use rootsignal_common::{EvidenceNode, Node, NodeType};
use rootsignal_graph::{GraphWriter, GraphClient};

use crate::embedder::Embedder;
use crate::extractor::Extractor;
use crate::quality;
use crate::scraper::{self, PageScraper, TavilySearcher};
use crate::sources;

/// Stats from a scout run.
#[derive(Debug, Default)]
pub struct ScoutStats {
    pub urls_scraped: u32,
    pub urls_failed: u32,
    pub signals_extracted: u32,
    pub signals_rejected_pii: u32,
    pub signals_deduplicated: u32,
    pub signals_stored: u32,
    pub by_type: [u32; 4], // Event, Give, Ask, Tension
    pub fresh_7d: u32,
    pub fresh_30d: u32,
    pub fresh_90d: u32,
    pub audience_roles: std::collections::HashMap<String, u32>,
}

impl std::fmt::Display for ScoutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Scout Run Complete ===")?;
        writeln!(f, "URLs scraped:       {}", self.urls_scraped)?;
        writeln!(f, "URLs failed:        {}", self.urls_failed)?;
        writeln!(f, "Signals extracted:  {}", self.signals_extracted)?;
        writeln!(f, "Signals rejected:   {} (PII)", self.signals_rejected_pii)?;
        writeln!(f, "Signals deduped:    {}", self.signals_deduplicated)?;
        writeln!(f, "Signals stored:     {}", self.signals_stored)?;
        writeln!(f, "\nBy type:")?;
        writeln!(f, "  Event:   {}", self.by_type[0])?;
        writeln!(f, "  Give:    {}", self.by_type[1])?;
        writeln!(f, "  Ask:     {}", self.by_type[2])?;
        writeln!(f, "  Tension: {}", self.by_type[3])?;
        let total = self.signals_stored.max(1);
        writeln!(f, "\nFreshness:")?;
        writeln!(f, "  < 7 days:   {} ({:.0}%)", self.fresh_7d, self.fresh_7d as f64 / total as f64 * 100.0)?;
        writeln!(f, "  7-30 days:  {} ({:.0}%)", self.fresh_30d, self.fresh_30d as f64 / total as f64 * 100.0)?;
        writeln!(f, "  30-90 days: {} ({:.0}%)", self.fresh_90d, self.fresh_90d as f64 / total as f64 * 100.0)?;
        writeln!(f, "\nAudience roles:")?;
        let mut roles: Vec<_> = self.audience_roles.iter().collect();
        roles.sort_by(|a, b| b.1.cmp(a.1));
        for (role, count) in roles {
            writeln!(f, "  {}: {}", role, count)?;
        }
        Ok(())
    }
}

pub struct Scout {
    writer: GraphWriter,
    extractor: Extractor,
    embedder: Embedder,
    scraper: Box<dyn PageScraper>,
    tavily: TavilySearcher,
}

impl Scout {
    pub fn new(
        graph_client: GraphClient,
        anthropic_api_key: &str,
        voyage_api_key: &str,
        firecrawl_api_key: &str,
        tavily_api_key: &str,
    ) -> Result<Self> {
        Ok(Self {
            writer: GraphWriter::new(graph_client),
            extractor: Extractor::new(anthropic_api_key),
            embedder: Embedder::new(voyage_api_key),
            scraper: scraper::build_scraper(firecrawl_api_key)?,
            tavily: TavilySearcher::new(tavily_api_key),
        })
    }

    /// Run a full scout cycle.
    pub async fn run(&self) -> Result<ScoutStats> {
        // Acquire lock
        if !self.writer.acquire_scout_lock().await.context("Failed to check scout lock")? {
            anyhow::bail!("Another scout run is in progress");
        }

        let result = self.run_inner().await;

        // Always release lock
        if let Err(e) = self.writer.release_scout_lock().await {
            error!("Failed to release scout lock: {e}");
        }

        result
    }

    async fn run_inner(&self) -> Result<ScoutStats> {
        let mut stats = ScoutStats::default();
        let mut all_urls: Vec<(String, f32)> = Vec::new();

        // 1. Tavily searches (parallel, 5 at a time)
        info!("Starting Tavily searches...");
        let queries = sources::tavily_queries();
        let search_results: Vec<_> = stream::iter(queries.into_iter().map(|query| {
            async move {
                (query, self.tavily.search(query, 5).await)
            }
        }))
        .buffer_unordered(5)
        .collect()
        .await;

        for (query, result) in search_results {
            match result {
                Ok(results) => {
                    for r in results {
                        let trust = sources::source_trust(&r.url);
                        all_urls.push((r.url, trust));
                    }
                }
                Err(e) => {
                    warn!(query, error = %e, "Tavily search failed");
                }
            }
        }

        // 2. Curated sources
        info!("Adding curated sources...");
        for (url, trust) in sources::curated_sources() {
            all_urls.push((url.to_string(), trust));
        }

        // Deduplicate URLs
        all_urls.sort_by(|a, b| a.0.cmp(&b.0));
        all_urls.dedup_by(|a, b| a.0 == b.0);
        info!(total_urls = all_urls.len(), "Unique URLs to scrape");

        // 3. Scrape + extract in parallel (5 concurrent), then write to graph sequentially
        let pipeline_results: Vec<_> = stream::iter(all_urls.iter().map(|(url, source_trust)| {
            let url = url.clone();
            let source_trust = *source_trust;
            async move {
                // Scrape
                let content = match self.scraper.scrape(&url).await {
                    Ok(c) if !c.is_empty() => c,
                    Ok(_) => return (url, source_trust, None),
                    Err(e) => {
                        warn!(url, error = %e, "Scrape failed");
                        return (url, source_trust, None);
                    }
                };
                // Extract (LLM call)
                match self.extractor.extract(&content, &url, source_trust).await {
                    Ok(nodes) => (url, source_trust, Some((content, nodes))),
                    Err(e) => {
                        warn!(url, error = %e, "Failed to process URL");
                        (url, source_trust, None)
                    }
                }
            }
        }))
        .buffer_unordered(5)
        .collect()
        .await;

        // Process extracted nodes sequentially (batch embed + dedup + graph writes)
        for (url, source_trust, result) in pipeline_results {
            match result {
                Some((content, nodes)) => {
                    match self.store_signals(&url, &content, nodes, &mut stats).await {
                        Ok(_) => stats.urls_scraped += 1,
                        Err(e) => {
                            warn!(url, error = %e, "Failed to store signals");
                            stats.urls_failed += 1;
                        }
                    }
                }
                None => stats.urls_failed += 1,
            }
        }

        info!("{stats}");
        Ok(stats)
    }

    async fn store_signals(
        &self,
        url: &str,
        content: &str,
        mut nodes: Vec<Node>,
        stats: &mut ScoutStats,
    ) -> Result<()> {
        stats.signals_extracted += nodes.len() as u32;

        // Score quality and set confidence
        for node in &mut nodes {
            let q = quality::score(node);
            if let Some(meta) = node_meta_mut(node) {
                meta.confidence = q.confidence;
            }
        }

        // Filter to signal nodes only (skip Evidence)
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| !matches!(n.node_type(), NodeType::Evidence))
            .collect();

        if nodes.is_empty() {
            return Ok(());
        }

        // Batch embed all signals at once (1 API call instead of N)
        let embed_texts: Vec<String> = nodes
            .iter()
            .map(|n| {
                format!(
                    "{} {}",
                    n.title(),
                    n.meta().map(|m| m.summary.as_str()).unwrap_or("")
                )
            })
            .collect();

        let embeddings = match self.embedder.embed_batch(embed_texts).await {
            Ok(e) => e,
            Err(e) => {
                warn!(url, error = %e, "Batch embedding failed, skipping all signals");
                return Ok(());
            }
        };

        // Process each signal with its pre-computed embedding
        let now = Utc::now();
        let content_hash = format!("{:x}", md5_hash(content));

        for (node, embedding) in nodes.into_iter().zip(embeddings.into_iter()) {
            let node_type = node.node_type();
            let type_idx = match node_type {
                NodeType::Event => 0,
                NodeType::Give => 1,
                NodeType::Ask => 2,
                NodeType::Tension => 3,
                NodeType::Evidence => continue,
            };

            // Dedup check
            match self.writer.find_duplicate(&embedding, node_type, 0.92).await {
                Ok(Some((existing_id, score))) => {
                    info!(
                        existing_id = %existing_id,
                        score,
                        title = node.title(),
                        "Duplicate found, corroborating"
                    );
                    self.writer.corroborate(existing_id, node_type, now).await?;

                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.to_string(),
                        retrieved_at: now,
                        content_hash: content_hash.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                    };
                    self.writer.create_evidence(&evidence, existing_id).await?;

                    stats.signals_deduplicated += 1;
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(error = %e, "Dedup check failed, proceeding with creation");
                }
            }

            // Create new node
            let node_id = self.writer.create_node(&node, &embedding).await?;

            let evidence = EvidenceNode {
                id: Uuid::new_v4(),
                source_url: url.to_string(),
                retrieved_at: now,
                content_hash: content_hash.clone(),
                snippet: node.meta().map(|m| m.summary.clone()),
            };
            self.writer.create_evidence(&evidence, node_id).await?;

            // Update stats
            stats.signals_stored += 1;
            stats.by_type[type_idx] += 1;

            // Freshness bucketing based on extracted_at
            if let Some(meta) = node.meta() {
                let age = now - meta.extracted_at;
                if age.num_days() < 7 {
                    stats.fresh_7d += 1;
                } else if age.num_days() < 30 {
                    stats.fresh_30d += 1;
                } else if age.num_days() < 90 {
                    stats.fresh_90d += 1;
                }

                for role in &meta.audience_roles {
                    *stats
                        .audience_roles
                        .entry(format!("{role}"))
                        .or_insert(0) += 1;
                }
            }
        }

        Ok(())
    }
}

fn node_meta_mut(node: &mut Node) -> Option<&mut rootsignal_common::NodeMeta> {
    match node {
        Node::Event(n) => Some(&mut n.meta),
        Node::Give(n) => Some(&mut n.meta),
        Node::Ask(n) => Some(&mut n.meta),
        Node::Tension(n) => Some(&mut n.meta),
        Node::Evidence(_) => None,
    }
}

/// Simple hash for content dedup. Not cryptographic.
fn md5_hash(content: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}
