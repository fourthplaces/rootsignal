use std::collections::{HashMap, HashSet};

use chrono::Utc;
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{ClusterSnapshot, EntityMappingOwned, StoryNode};

use crate::{GraphClient, SimilarityBuilder};
use crate::synthesizer::{Synthesizer, SynthesisInput};
use crate::writer::GraphWriter;

/// Minimum number of connected signals (with at least one SIMILAR_TO edge)
/// before running Leiden. Below this, clustering produces noise.
const MIN_CONNECTED_SIGNALS: u64 = 10;

/// Minimum containment ratio for asymmetric story reconciliation.
/// If |old ∩ new| / |old| >= this, it's the same story evolving.
const CONTAINMENT_THRESHOLD: f64 = 0.5;

/// Orchestrates the full clustering pipeline:
/// 1. Build similarity edges
/// 2. Run Leiden community detection
/// 3. Reconcile with existing stories (asymmetric containment)
/// 4. Generate LLM headlines for new stories
/// 5. Compute velocity and energy
pub struct Clusterer {
    client: GraphClient,
    writer: GraphWriter,
    anthropic_api_key: String,
    entity_mappings: Vec<EntityMappingOwned>,
}


/// A community detected by Leiden.
struct Community {
    id: i64,
    signal_ids: Vec<String>,
}

impl Clusterer {
    pub fn new(
        client: GraphClient,
        anthropic_api_key: &str,
        entity_mappings: Vec<EntityMappingOwned>,
    ) -> Self {
        Self {
            writer: GraphWriter::new(client.clone()),
            client,
            anthropic_api_key: anthropic_api_key.to_string(),
            entity_mappings,
        }
    }

    /// Run the full clustering pipeline.
    pub async fn run(&self) -> Result<ClusterStats, neo4rs::Error> {
        let mut stats = ClusterStats::default();

        // 1. Build similarity edges
        let similarity = SimilarityBuilder::new(self.client.clone());
        similarity.clear_edges().await?;
        let edges_created = similarity.build_edges().await?;
        stats.similarity_edges = edges_created;

        if edges_created == 0 {
            info!("No similarity edges created, skipping clustering");
            stats.status = "insufficient_signals".to_string();
            return Ok(stats);
        }

        // Check minimum connected signals
        let connected = self.count_connected_signals().await?;
        if connected < MIN_CONNECTED_SIGNALS {
            info!(connected, min = MIN_CONNECTED_SIGNALS, "Insufficient connected signals for clustering");
            stats.status = "insufficient_signals".to_string();
            return Ok(stats);
        }

        // 2. Run Leiden community detection
        let communities = self.run_leiden().await?;
        info!(communities = communities.len(), "Leiden communities detected");

        if communities.is_empty() {
            stats.status = "no_communities".to_string();
            return Ok(stats);
        }

        // 3. Get existing stories for reconciliation
        let existing_stories = self.writer.get_existing_stories().await?;

        // 4. Reconcile communities with existing stories
        let now = Utc::now();
        for community in &communities {
            if community.signal_ids.len() < 2 {
                continue; // Skip singleton clusters
            }

            let new_ids: HashSet<&str> = community.signal_ids.iter().map(|s| s.as_str()).collect();

            // Check asymmetric containment against existing stories
            let mut matched_story: Option<Uuid> = None;
            for (story_id, old_signal_ids) in &existing_stories {
                if old_signal_ids.is_empty() {
                    continue;
                }
                let old_set: HashSet<&str> = old_signal_ids.iter().map(|s| s.as_str()).collect();
                let intersection = old_set.intersection(&new_ids).count();
                let containment = intersection as f64 / old_set.len() as f64;
                if containment >= CONTAINMENT_THRESHOLD {
                    matched_story = Some(*story_id);
                    break;
                }
            }

            // Gather signal metadata for story properties
            let signal_meta = self.fetch_signal_metadata(&community.signal_ids).await?;

            // Compute org count and source diversity
            let source_urls: Vec<&str> = signal_meta.iter().map(|s| s.source_url.as_str()).collect();
            let source_domains: Vec<String> = source_urls
                .iter()
                .map(|u| rootsignal_common::extract_domain(u))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let entity_count = self.count_distinct_entities(&source_urls);
            let corroboration_depth = signal_meta.iter().filter(|s| s.corroboration_count > 0).count() as u32;

            // Status: confirmed if multi-entity, emerging if single-entity
            let status = if entity_count >= 2 { "confirmed" } else { "emerging" };

            // Dominant type and type diversity
            let mut type_counts: HashMap<String, u32> = HashMap::new();
            for meta in &signal_meta {
                *type_counts.entry(meta.node_type.clone()).or_insert(0) += 1;
            }
            let type_diversity = type_counts.len() as u32;
            let dominant_type = type_counts
                .iter()
                .max_by_key(|(_, c)| *c)
                .map(|(t, _)| t.clone())
                .unwrap_or_else(|| "notice".to_string());

            // Sensitivity (highest)
            let sensitivity = signal_meta
                .iter()
                .map(|s| s.sensitivity.as_str())
                .max_by_key(|s| match *s {
                    "sensitive" => 2,
                    "elevated" => 1,
                    _ => 0,
                })
                .unwrap_or("general")
                .to_string();

            // Centroid (average of signals with locations)
            let lats: Vec<f64> = signal_meta.iter().filter_map(|s| s.lat).collect();
            let lngs: Vec<f64> = signal_meta.iter().filter_map(|s| s.lng).collect();
            let (centroid_lat, centroid_lng) = if !lats.is_empty() {
                (
                    Some(lats.iter().sum::<f64>() / lats.len() as f64),
                    Some(lngs.iter().sum::<f64>() / lngs.len() as f64),
                )
            } else {
                (None, None)
            };

            if let Some(story_id) = matched_story {
                // Update existing story
                let story = StoryNode {
                    id: story_id,
                    headline: String::new(), // Will be preserved from existing
                    summary: String::new(),
                    signal_count: community.signal_ids.len() as u32,
                    first_seen: now, // Preserved from existing
                    last_updated: now,
                    velocity: 0.0,
                    energy: 0.0,
                    centroid_lat,
                    centroid_lng,
                    dominant_type,

                    sensitivity,
                    source_count: source_domains.len() as u32,
                    entity_count,
                    type_diversity,
                    source_domains,
                    corroboration_depth,
                    status: status.to_string(),
                    arc: None,
                    category: None,
                    lede: None,
                    narrative: None,
                    action_guidance: None,
                };

                // Partial update (preserve headline/summary/first_seen)
                self.update_story_preserving(&story).await?;

                // Rebuild CONTAINS links
                self.writer.clear_story_signals(story_id).await?;
                for sig_id in &community.signal_ids {
                    if let Ok(uuid) = Uuid::parse_str(sig_id) {
                        self.writer.link_signal_to_story(story_id, uuid).await?;
                    }
                }

                stats.stories_updated += 1;
            } else {
                // Generate headline + summary for new story
                let (headline, summary) = self.label_cluster(&signal_meta).await;

                let story_id = Uuid::new_v4();
                let story = StoryNode {
                    id: story_id,
                    headline,
                    summary,
                    signal_count: community.signal_ids.len() as u32,
                    first_seen: now,
                    last_updated: now,
                    velocity: 0.0,
                    energy: 0.0,
                    centroid_lat,
                    centroid_lng,
                    dominant_type,

                    sensitivity,
                    source_count: source_domains.len() as u32,
                    entity_count,
                    type_diversity,
                    source_domains,
                    corroboration_depth,
                    status: status.to_string(),
                    arc: None,
                    category: None,
                    lede: None,
                    narrative: None,
                    action_guidance: None,
                };

                self.writer.create_story(&story).await?;
                for sig_id in &community.signal_ids {
                    if let Ok(uuid) = Uuid::parse_str(sig_id) {
                        self.writer.link_signal_to_story(story_id, uuid).await?;
                    }
                }

                stats.stories_created += 1;
            }
        }

        // 5. Run story synthesis on created/updated stories (non-fatal)
        self.synthesize_stories().await;

        // 6. Create snapshots and compute velocity/energy for all active stories
        self.compute_velocity_and_energy().await?;

        stats.status = "complete".to_string();
        info!(
            edges = stats.similarity_edges,
            created = stats.stories_created,
            updated = stats.stories_updated,
            "Clustering complete"
        );

        Ok(stats)
    }

    /// Count signals that have at least one SIMILAR_TO edge.
    async fn count_connected_signals(&self) -> Result<u64, neo4rs::Error> {
        let q = query(
            "MATCH (n)-[:SIMILAR_TO]-()
             WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
             RETURN count(DISTINCT n) AS cnt"
        );

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u64);
        }

        Ok(0)
    }

    /// Run Leiden community detection on the SIMILAR_TO subgraph.
    async fn run_leiden(&self) -> Result<Vec<Community>, neo4rs::Error> {
        let q = query(
            "CALL leiden_community_detection.get('weight', 1.0)
             YIELD node, community_id
             RETURN node.id AS signal_id, community_id"
        );

        let mut communities_map: HashMap<i64, Vec<String>> = HashMap::new();

        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let signal_id: String = row.get("signal_id").unwrap_or_default();
            let community_id: i64 = row.get("community_id").unwrap_or(-1);
            if !signal_id.is_empty() && community_id >= 0 {
                communities_map
                    .entry(community_id)
                    .or_default()
                    .push(signal_id);
            }
        }

        Ok(communities_map
            .into_iter()
            .map(|(id, signal_ids)| Community { id, signal_ids })
            .collect())
    }

    /// Generate headline + summary for a new cluster using LLM.
    async fn label_cluster(&self, signals: &[SignalMeta]) -> (String, String) {
        let signal_descriptions: Vec<String> = signals
            .iter()
            .take(15) // Don't send too many to Haiku
            .map(|s| format!("- [{}] {}: {}", s.node_type, s.title, s.summary))
            .collect();

        let prompt = format!(
            r#"These signals were grouped by semantic similarity. The system discovered this grouping — no categories were predefined.

Signals in this cluster:
{}

Generate:
1. A specific headline (max 80 chars) that describes the pattern. Do NOT use generic category labels like "Community events" or "Housing issues". The headline must distinguish this cluster from any other.
2. A 2-3 sentence summary of what these signals collectively represent.

Respond in this exact JSON format:
{{"headline": "...", "summary": "..."}}"#,
            signal_descriptions.join("\n")
        );

        // Use ai_client to call Haiku
        match self.call_haiku(&prompt).await {
            Ok((headline, summary)) => (headline, summary),
            Err(e) => {
                warn!(error = %e, "LLM headline generation failed, using fallback");
                let fallback_headline = signals
                    .first()
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| "Emerging pattern".to_string());
                (fallback_headline, "Cluster of related civic signals.".to_string())
            }
        }
    }

    /// Call Haiku for headline generation.
    async fn call_haiku(&self, prompt: &str) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        use ai_client::claude::Claude;

        let claude = Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");
        let response = claude.chat_completion(
            "You are a concise headline writer for a civic signal system. Respond only with valid JSON.",
            prompt,
        ).await?;

        // Parse JSON response
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        let headline = parsed["headline"].as_str().unwrap_or("Emerging pattern").to_string();
        let summary = parsed["summary"].as_str().unwrap_or("Cluster of related civic signals.").to_string();

        Ok((headline, summary))
    }

    /// Count distinct entities across source URLs.
    fn count_distinct_entities(&self, source_urls: &[&str]) -> u32 {
        let mut entities = HashSet::new();
        for url in source_urls {
            let entity = rootsignal_common::resolve_entity(url, &self.entity_mappings);
            entities.insert(entity);
        }
        entities.len() as u32
    }

    /// Fetch metadata for a set of signal IDs.
    async fn fetch_signal_metadata(&self, signal_ids: &[String]) -> Result<Vec<SignalMeta>, neo4rs::Error> {
        let mut results = Vec::new();

        for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE n.id IN $ids
                 RETURN n.id AS id, n.title AS title, n.summary AS summary,
                        n.source_url AS source_url, n.corroboration_count AS corroboration_count,
                        n.sensitivity AS sensitivity,
                        n.lat AS lat, n.lng AS lng"
            ))
            .param("ids", signal_ids.to_vec());

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id: String = row.get("id").unwrap_or_default();
                let title: String = row.get("title").unwrap_or_default();
                let summary: String = row.get("summary").unwrap_or_default();
                let source_url: String = row.get("source_url").unwrap_or_default();
                let corroboration_count: i64 = row.get("corroboration_count").unwrap_or(0);
                let sensitivity: String = row.get("sensitivity").unwrap_or_else(|_| "general".to_string());
                let lat: Option<f64> = row.get("lat").ok().and_then(|v: f64| if v == 0.0 { None } else { Some(v) });
                let lng: Option<f64> = row.get("lng").ok().and_then(|v: f64| if v == 0.0 { None } else { Some(v) });

                results.push(SignalMeta {
                    id,
                    title,
                    summary,
                    source_url,
                    node_type: label.to_lowercase(),
                    corroboration_count: corroboration_count as u32,

                    sensitivity,
                    lat,
                    lng,
                });
            }
        }

        Ok(results)
    }

    /// Update story fields but preserve headline/summary/first_seen from existing.
    async fn update_story_preserving(&self, story: &StoryNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})
             SET s.signal_count = $signal_count,
                 s.last_updated = datetime($last_updated),
                 s.centroid_lat = $centroid_lat,
                 s.centroid_lng = $centroid_lng,
                 s.dominant_type = $dominant_type,

                 s.sensitivity = $sensitivity,
                 s.source_count = $source_count,
                 s.entity_count = $entity_count,
                 s.type_diversity = $type_diversity,
                 s.source_domains = $source_domains,
                 s.corroboration_depth = $corroboration_depth,
                 s.status = $status"
        )
        .param("id", story.id.to_string())
        .param("signal_count", story.signal_count as i64)
        .param("last_updated", crate::writer::memgraph_datetime_pub(&story.last_updated))
        .param("dominant_type", story.dominant_type.as_str())

        .param("sensitivity", story.sensitivity.as_str())
        .param("source_count", story.source_count as i64)
        .param("entity_count", story.entity_count as i64)
        .param("type_diversity", story.type_diversity as i64)
        .param("source_domains", story.source_domains.clone())
        .param("corroboration_depth", story.corroboration_depth as i64)
        .param("status", story.status.as_str());

        let q = match (story.centroid_lat, story.centroid_lng) {
            (Some(lat), Some(lng)) => q.param("centroid_lat", lat).param("centroid_lng", lng),
            _ => q.param::<Option<f64>>("centroid_lat", None).param::<Option<f64>>("centroid_lng", None),
        };

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Compute velocity and energy for all active stories and create snapshots.
    async fn compute_velocity_and_energy(&self) -> Result<(), neo4rs::Error> {
        let now = Utc::now();

        // Get all stories with current signal counts and entity counts
        let q = query(
            "MATCH (s:Story)
             OPTIONAL MATCH (s)-[:CONTAINS]->(n)
             RETURN s.id AS id, s.source_count AS source_count,
                    s.entity_count AS entity_count, count(n) AS signal_count"
        );

        let mut stories: Vec<(Uuid, u32, u32, u32)> = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let source_count: i64 = row.get("source_count").unwrap_or(0);
            let entity_count: i64 = row.get("entity_count").unwrap_or(0);
            let signal_count: i64 = row.get("signal_count").unwrap_or(0);
            if let Ok(id) = Uuid::parse_str(&id_str) {
                stories.push((id, signal_count as u32, source_count as u32, entity_count as u32));
            }
        }

        for (story_id, current_count, source_count, entity_count) in stories {
            // Create snapshot with entity_count for velocity tracking
            let snapshot = ClusterSnapshot {
                id: Uuid::new_v4(),
                story_id,
                signal_count: current_count,
                entity_count,
                run_at: now,
            };
            self.writer.create_cluster_snapshot(&snapshot).await?;

            // Velocity driven by entity diversity growth, not raw signal count.
            // A flood from one source doesn't move the needle.
            let entity_count_7d_ago = self.writer.get_snapshot_entity_count_7d_ago(story_id).await?;
            let velocity = match entity_count_7d_ago {
                Some(old_entities) => (entity_count as f64 - old_entities as f64) / 7.0,
                None => entity_count as f64 / 7.0, // First run, assume all new
            };

            // Recency score: 1.0 today -> 0.0 at 14 days
            let recency_score = {
                let last_updated_str = self.get_story_last_updated(story_id).await?;
                parse_recency(&last_updated_str, &now)
            };

            // Source diversity: min(unique_source_urls / 5.0, 1.0)
            let source_diversity = (source_count as f64 / 5.0).min(1.0);

            // Energy = velocity * 0.5 + recency * 0.3 + source_diversity * 0.2
            let energy = velocity * 0.5 + recency_score * 0.3 + source_diversity * 0.2;

            // Update story velocity and energy
            let q = query(
                "MATCH (s:Story {id: $id})
                 SET s.velocity = $velocity, s.energy = $energy"
            )
            .param("id", story_id.to_string())
            .param("velocity", velocity)
            .param("energy", energy);

            self.client.graph.run(q).await?;
        }

        Ok(())
    }

    /// Run synthesis on all stories that don't yet have a lede.
    /// Non-fatal: logs errors but doesn't fail the pipeline.
    async fn synthesize_stories(&self) {
        let synthesizer = Synthesizer::new(&self.anthropic_api_key);

        // Find stories without synthesis
        let q = query(
            "MATCH (s:Story)
             WHERE s.lede IS NULL OR s.lede = ''
             RETURN s.id AS id, s.headline AS headline, s.velocity AS velocity,
                    s.first_seen AS first_seen"
        );

        let stories_to_synth: Vec<(Uuid, String, f64, String)> = match self.client.graph.execute(q).await {
            Ok(mut stream) => {
                let mut results = Vec::new();
                while let Ok(Some(row)) = stream.next().await {
                    let id_str: String = row.get("id").unwrap_or_default();
                    if let Ok(id) = Uuid::parse_str(&id_str) {
                        let headline: String = row.get("headline").unwrap_or_default();
                        let velocity: f64 = row.get("velocity").unwrap_or(0.0);
                        let first_seen: String = row.get("first_seen").unwrap_or_default();
                        results.push((id, headline, velocity, first_seen));
                    }
                }
                results
            }
            Err(e) => {
                warn!(error = %e, "Failed to find stories for synthesis");
                return;
            }
        };

        if stories_to_synth.is_empty() {
            return;
        }

        info!(count = stories_to_synth.len(), "Running story synthesis");

        for (story_id, headline, velocity, first_seen_str) in &stories_to_synth {
            // Get constituent signals for this story
            let signal_ids = match self.get_story_signal_ids(*story_id).await {
                Ok(ids) => ids,
                Err(e) => {
                    warn!(story_id = %story_id, error = %e, "Failed to get story signals for synthesis");
                    continue;
                }
            };

            let signal_meta = match self.fetch_signal_metadata(&signal_ids).await {
                Ok(meta) => meta,
                Err(e) => {
                    warn!(story_id = %story_id, error = %e, "Failed to fetch signal metadata for synthesis");
                    continue;
                }
            };

            let inputs: Vec<SynthesisInput> = signal_meta
                .iter()
                .map(|s| SynthesisInput {
                    title: s.title.clone(),
                    summary: s.summary.clone(),
                    node_type: s.node_type.clone(),
                    source_url: s.source_url.clone(),
                    action_url: None,
                })
                .collect();

            let age_days = {
                use chrono::NaiveDateTime;
                let dt = if let Ok(naive) = NaiveDateTime::parse_from_str(first_seen_str, "%Y-%m-%dT%H:%M:%S%.f") {
                    naive.and_utc()
                } else {
                    Utc::now()
                };
                (Utc::now() - dt).num_hours() as f64 / 24.0
            };

            match synthesizer.synthesize(headline, &inputs, *velocity, age_days).await {
                Ok(synthesis) => {
                    let action_guidance_json = serde_json::to_string(&synthesis.action_guidance).unwrap_or_default();
                    if let Err(e) = self.writer.update_story_synthesis(
                        *story_id,
                        &synthesis.headline,
                        &synthesis.lede,
                        &synthesis.narrative,
                        &synthesis.arc.to_string(),
                        &synthesis.category.to_string(),
                        &action_guidance_json,
                    ).await {
                        warn!(story_id = %story_id, error = %e, "Failed to write story synthesis");
                    }
                }
                Err(e) => {
                    warn!(story_id = %story_id, error = %e, "Story synthesis LLM call failed");
                }
            }
        }
    }

    /// Get signal IDs for a story.
    async fn get_story_signal_ids(&self, story_id: Uuid) -> Result<Vec<String>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(n)
             RETURN n.id AS id"
        )
        .param("id", story_id.to_string());

        let mut ids = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            if !id.is_empty() {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    async fn get_story_last_updated(&self, story_id: Uuid) -> Result<String, neo4rs::Error> {
        let q = query("MATCH (s:Story {id: $id}) RETURN s.last_updated AS last_updated")
            .param("id", story_id.to_string());
        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            return Ok(row.get::<String>("last_updated").unwrap_or_default());
        }
        Ok(String::new())
    }
}

/// Signal metadata for clustering operations.
struct SignalMeta {
    id: String,
    title: String,
    summary: String,
    source_url: String,
    node_type: String,
    corroboration_count: u32,
    sensitivity: String,
    lat: Option<f64>,
    lng: Option<f64>,
}

#[derive(Debug, Default)]
pub struct ClusterStats {
    pub similarity_edges: u64,
    pub stories_created: u32,
    pub stories_updated: u32,
    pub status: String,
}

impl std::fmt::Display for ClusterStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Clustering Complete ===")?;
        writeln!(f, "Similarity edges: {}", self.similarity_edges)?;
        writeln!(f, "Stories created:  {}", self.stories_created)?;
        writeln!(f, "Stories updated:  {}", self.stories_updated)?;
        writeln!(f, "Status:           {}", self.status)?;
        Ok(())
    }
}

/// Parse a datetime string and compute recency score: 1.0 today → 0.0 at 14+ days.
fn parse_recency(datetime_str: &str, now: &chrono::DateTime<Utc>) -> f64 {
    use chrono::NaiveDateTime;

    let dt: chrono::DateTime<Utc> = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(datetime_str) {
        dt.with_timezone(&Utc)
    } else if let Ok(naive) = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M:%S%.f") {
        naive.and_utc()
    } else {
        return 0.0_f64; // Can't parse → treat as stale
    };

    let age_days: f64 = (*now - dt).num_hours() as f64 / 24.0;
    (1.0_f64 - age_days / 14.0_f64).clamp(0.0_f64, 1.0_f64)
}

