//! StoryWeaver: Emergent, Anti-Fragile Story Generation
//!
//! Stories are graph patterns, not created objects. A story already exists in the graph
//! the moment a tension accumulates 2+ responding signals from distinct sources.
//! The `Story` node is a **materialized view** — a cached rendering of a graph pattern
//! for the API/UI layer. The source of truth is the tension hub itself.
//!
//! Three-phase pipeline:
//! - **Phase A (Materialize):** Tension hubs with 2+ respondents become Story nodes.
//!   No LLM needed — the tension title IS the headline, signals ARE the narrative.
//! - **Phase B (Grow):** Existing stories absorb new RESPONDS_TO edges. Stories accrete.
//! - **Phase C (Enrich):** Budget-gated LLM synthesis. Stories exist immediately; synthesis
//!   runs when budget allows.
//!
//! Anti-fragility properties:
//! - Contradictions surface (mixed signal types → multi-perspective synthesis)
//! - Investigation failures become coverage gap intelligence (abandoned count)
//! - Resurgence is a named arc (fading story + new activity → Resurgent)
//! - Budget exhaustion degrades gracefully (Phase A/B always run)

use std::collections::HashSet;

use chrono::Utc;
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{extract_domain, StoryNode};

use crate::story_metrics::{parse_recency, story_energy, story_status};
use crate::synthesizer::{SynthesisInput, Synthesizer};
use crate::writer::GraphWriter;
use crate::GraphClient;

/// Minimum containment ratio for absorbing a tension hub into an existing story.
/// If |hub_signals ∩ story_signals| / |hub_signals| >= this, absorb rather than create new.
const CONTAINMENT_THRESHOLD: f64 = 0.5;

/// Maximum respondents before flagging a tension hub as needing refinement.
const MEGA_TENSION_THRESHOLD: usize = 30;

pub struct StoryWeaver {
    client: GraphClient,
    writer: GraphWriter,
    anthropic_api_key: String,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
}

/// Stats from a StoryWeaver run.
#[derive(Debug, Default)]
pub struct StoryWeaverStats {
    pub stories_materialized: u32,
    pub stories_grown: u32,
    pub stories_enriched: u32,
    pub signals_linked: u32,
    pub stories_absorbed: u32,
    pub abandoned_signals: u32,
}

impl std::fmt::Display for StoryWeaverStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StoryWeaver: {} materialized, {} grown, {} enriched, \
             {} signals linked, {} absorbed, {} abandoned",
            self.stories_materialized,
            self.stories_grown,
            self.stories_enriched,
            self.signals_linked,
            self.stories_absorbed,
            self.abandoned_signals,
        )
    }
}

impl StoryWeaver {
    pub fn new(
        client: GraphClient,
        anthropic_api_key: &str,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    ) -> Self {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * center_lat.to_radians().cos());
        Self {
            writer: GraphWriter::new(client.clone()),
            client,
            anthropic_api_key: anthropic_api_key.to_string(),
            min_lat: center_lat - lat_delta,
            max_lat: center_lat + lat_delta,
            min_lng: center_lng - lng_delta,
            max_lng: center_lng + lng_delta,
        }
    }

    /// Run the three-phase story weaving pipeline.
    ///
    /// `has_enrichment_budget` controls whether Phase C (LLM synthesis) runs.
    /// Phases A and B always run — they're cheap graph queries + writes.
    pub async fn run(
        &self,
        has_enrichment_budget: bool,
    ) -> Result<StoryWeaverStats, neo4rs::Error> {
        let mut stats = StoryWeaverStats::default();

        // Count abandoned signals for coverage gap reporting
        stats.abandoned_signals = self.writer.count_abandoned_signals().await.unwrap_or(0);
        if stats.abandoned_signals > 0 {
            warn!(
                count = stats.abandoned_signals,
                "Coverage gap: abandoned curiosity investigations (3+ failures)"
            );
        }

        // Phase A: Materialize tension hubs as stories
        self.phase_materialize(&mut stats).await?;

        // Phase B: Grow existing stories with new respondents
        self.phase_grow(&mut stats).await?;

        // Phase C: Enrich stories with LLM synthesis (budget-gated)
        if has_enrichment_budget {
            self.phase_enrich(&mut stats).await;
        } else {
            info!("Phase C (enrichment) skipped — no budget");
        }

        // Phase D: Compute velocity, energy, gap_velocity; snapshot; archive zombies
        self.phase_velocity_energy().await?;

        Ok(stats)
    }

    /// Phase A: Materialize — create Story nodes from tension hubs with 2+ respondents.
    async fn phase_materialize(&self, stats: &mut StoryWeaverStats) -> Result<(), neo4rs::Error> {
        let hubs = self
            .writer
            .find_tension_hubs(10, self.min_lat, self.max_lat, self.min_lng, self.max_lng)
            .await?;
        if hubs.is_empty() {
            return Ok(());
        }

        info!(hubs = hubs.len(), "Phase A: materializing tension hubs");

        // Load existing stories for containment check
        let existing_stories = self.writer.get_existing_stories().await?;

        let now = Utc::now();

        for hub in &hubs {
            let hub_signal_ids: HashSet<String> = hub
                .respondents
                .iter()
                .map(|r| r.signal_id.to_string())
                .collect();

            // Containment check: do hub signals overlap with an existing story?
            let mut absorbed_into: Option<Uuid> = None;
            for (story_id, story_signal_ids) in &existing_stories {
                if story_signal_ids.is_empty() {
                    continue;
                }
                let story_set: HashSet<&str> =
                    story_signal_ids.iter().map(|s| s.as_str()).collect();
                let intersection = hub_signal_ids
                    .iter()
                    .filter(|id| story_set.contains(id.as_str()))
                    .count();
                let containment = intersection as f64 / hub_signal_ids.len() as f64;
                if containment >= CONTAINMENT_THRESHOLD {
                    absorbed_into = Some(*story_id);
                    break;
                }
            }

            if let Some(story_id) = absorbed_into {
                // Absorb: link the tension + remaining signals into the existing story
                self.writer
                    .link_signal_to_story(story_id, hub.tension_id)
                    .await?;
                for resp in &hub.respondents {
                    self.writer
                        .link_signal_to_story(story_id, resp.signal_id)
                        .await?;
                    stats.signals_linked += 1;
                }
                // Aggregate signal tags → story tags
                self.writer.aggregate_story_tags(story_id).await?;
                // Flag for re-synthesis
                self.set_synthesis_pending(story_id).await?;
                stats.stories_absorbed += 1;
                info!(
                    story_id = %story_id,
                    tension = hub.title.as_str(),
                    "Tension hub absorbed into existing story"
                );
                continue;
            }

            // Create new Story node from tension hub
            let story_id = Uuid::new_v4();

            // Compute metadata from respondent signals
            let source_urls: Vec<&str> = hub
                .respondents
                .iter()
                .map(|r| r.source_url.as_str())
                .collect();
            let source_domains: Vec<String> = source_urls
                .iter()
                .map(|u| extract_domain(u))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            // Fetch signal types for status computation
            let signal_ids: Vec<String> = hub
                .respondents
                .iter()
                .map(|r| r.signal_id.to_string())
                .collect();
            let type_diversity = self.count_type_diversity(&signal_ids).await.unwrap_or(1);

            let signal_count = hub.respondents.len() as u32;
            let entity_count = source_domains.len() as u32;
            let status = story_status(type_diversity, entity_count, signal_count as usize);

            let needs_refinement = hub.respondents.len() >= MEGA_TENSION_THRESHOLD;
            if needs_refinement {
                info!(
                    tension = hub.title.as_str(),
                    respondents = hub.respondents.len(),
                    "Mega-tension flagged for future refinement"
                );
            }

            // Compute centroid from respondent signals
            let signal_meta = self.fetch_signal_metadata(&signal_ids).await?;
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

            // Propagate max sensitivity from constituent signals
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

            // Propagate cause_heat from the central tension
            let cause_heat = signal_meta
                .iter()
                .filter(|s| s.node_type == "tension")
                .map(|s| s.cause_heat)
                .next()
                .unwrap_or(0.0);

            // Count signals by type and edge type
            let ask_count = signal_meta.iter().filter(|s| s.node_type == "ask").count() as u32;
            let give_count = signal_meta.iter().filter(|s| s.node_type == "give").count() as u32;
            let event_count = signal_meta.iter().filter(|s| s.node_type == "event").count() as u32;
            let drawn_to_count = hub
                .respondents
                .iter()
                .filter(|r| r.edge_type == "DRAWN_TO")
                .count() as u32;
            let gap_score = ask_count as i32 - give_count as i32;

            let story = StoryNode {
                id: story_id,
                headline: hub.title.clone(),
                summary: hub.summary.clone(),
                signal_count,
                first_seen: now,
                last_updated: now,
                velocity: 0.0,
                energy: 0.0,
                centroid_lat,
                centroid_lng,
                dominant_type: "tension".to_string(),
                sensitivity,
                source_count: source_domains.len() as u32,
                entity_count,
                type_diversity,
                source_domains,
                corroboration_depth: 0,
                status: status.to_string(),
                arc: None,
                category: hub.category.clone(),
                lede: None,
                narrative: None,
                action_guidance: None,
                cause_heat,
                ask_count,
                give_count,
                event_count,
                drawn_to_count,
                gap_score,
                gap_velocity: 0.0,
            };

            self.writer.create_story(&story).await?;

            // Link tension + respondent signals via CONTAINS
            self.writer
                .link_signal_to_story(story_id, hub.tension_id)
                .await?;
            for resp in &hub.respondents {
                self.writer
                    .link_signal_to_story(story_id, resp.signal_id)
                    .await?;
                stats.signals_linked += 1;
            }

            // Aggregate signal tags → story tags
            self.writer.aggregate_story_tags(story_id).await?;

            // Flag synthesis_pending and needs_refinement
            self.set_synthesis_pending(story_id).await?;
            if needs_refinement {
                self.set_needs_refinement(story_id).await?;
            }

            stats.stories_materialized += 1;
            info!(
                story_id = %story_id,
                headline = hub.title.as_str(),
                signals = signal_count,
                "Story materialized from tension hub"
            );
        }

        Ok(())
    }

    /// Phase B: Grow — add new respondent signals to existing stories.
    async fn phase_grow(&self, stats: &mut StoryWeaverStats) -> Result<(), neo4rs::Error> {
        let growths = self
            .writer
            .find_story_growth(20, self.min_lat, self.max_lat, self.min_lng, self.max_lng)
            .await?;
        if growths.is_empty() {
            return Ok(());
        }

        info!(stories = growths.len(), "Phase B: growing existing stories");

        for growth in &growths {
            for resp in &growth.new_respondents {
                self.writer
                    .link_signal_to_story(growth.story_id, resp.signal_id)
                    .await?;
                stats.signals_linked += 1;
            }

            // Refresh metadata
            self.refresh_story_metadata(growth.story_id).await?;

            // Re-aggregate signal tags → story tags
            self.writer.aggregate_story_tags(growth.story_id).await?;

            // Check for resurgence: was the story fading before this new activity?
            self.check_resurgence(growth.story_id).await?;

            // Flag for re-synthesis
            self.set_synthesis_pending(growth.story_id).await?;

            stats.stories_grown += 1;
            info!(
                story_id = %growth.story_id,
                new_signals = growth.new_respondents.len(),
                "Story grew with new respondents"
            );
        }

        Ok(())
    }

    /// Phase C: Enrich — LLM synthesis for stories that need it.
    async fn phase_enrich(&self, stats: &mut StoryWeaverStats) {
        let synthesizer = Synthesizer::new(&self.anthropic_api_key);

        // Find stories needing synthesis
        let q = query(
            "MATCH (s:Story)
             WHERE s.synthesis_pending = true OR s.lede IS NULL OR s.lede = ''
             RETURN s.id AS id, s.headline AS headline, s.velocity AS velocity,
                    s.first_seen AS first_seen, s.arc AS arc
             LIMIT 300",
        );

        let stories: Vec<(Uuid, String, f64, String, Option<String>)> =
            match self.client.graph.execute(q).await {
                Ok(mut stream) => {
                    let mut results = Vec::new();
                    while let Ok(Some(row)) = stream.next().await {
                        let id_str: String = row.get("id").unwrap_or_default();
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            let headline: String = row.get("headline").unwrap_or_default();
                            let velocity: f64 = row.get("velocity").unwrap_or(0.0);
                            let first_seen: String = row.get("first_seen").unwrap_or_default();
                            let arc: Option<String> = row.get("arc").ok();
                            results.push((id, headline, velocity, first_seen, arc));
                        }
                    }
                    results
                }
                Err(e) => {
                    warn!(error = %e, "Failed to find stories for enrichment");
                    return;
                }
            };

        if stories.is_empty() {
            return;
        }

        info!(count = stories.len(), "Phase C: enriching stories");

        for (story_id, headline, velocity, first_seen_str, prev_arc) in &stories {
            // Get signal metadata for this story
            let signal_ids = match self.get_story_signal_ids(*story_id).await {
                Ok(ids) => ids,
                Err(e) => {
                    warn!(story_id = %story_id, error = %e, "Failed to get story signals");
                    continue;
                }
            };

            let signal_meta = match self.fetch_signal_metadata(&signal_ids).await {
                Ok(meta) => meta,
                Err(e) => {
                    warn!(story_id = %story_id, error = %e, "Failed to fetch signal metadata");
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
                let dt = if let Ok(naive) =
                    NaiveDateTime::parse_from_str(first_seen_str, "%Y-%m-%dT%H:%M:%S%.f")
                {
                    naive.and_utc()
                } else {
                    Utc::now()
                };
                (Utc::now() - dt).num_hours() as f64 / 24.0
            };

            let was_fading = prev_arc.as_deref() == Some("fading");

            // Build editorial context for the synthesis prompt
            let mut context_parts: Vec<String> = Vec::new();

            if was_fading && *velocity > 0.0 {
                let quiet_days = age_days.round() as u64;
                context_parts.push(format!(
                    "This tension was quiet for approximately {quiet_days} days before new activity. \
                     This is a resurgence — note the return of attention."
                ));
            }

            // Check for signal type diversity (contradiction detection)
            let type_set: HashSet<&str> =
                signal_meta.iter().map(|s| s.node_type.as_str()).collect();
            if type_set.len() >= 2 {
                let has_tension = type_set.contains("tension");
                let has_response = type_set.contains("aid") || type_set.contains("gathering");
                if has_tension && has_response {
                    context_parts.push(
                        "This story includes both the underlying problem AND community responses. \
                         Surface both perspectives — the tension and the action being taken."
                            .to_string(),
                    );
                } else if type_set.len() >= 3 {
                    context_parts.push(
                        "Multiple signal types are present — surface the different perspectives \
                         rather than flattening them into a single narrative."
                            .to_string(),
                    );
                }
            }

            let extra_context = if context_parts.is_empty() {
                None
            } else {
                Some(context_parts.join("\n"))
            };

            match synthesizer
                .synthesize_with_context(
                    headline,
                    &inputs,
                    *velocity,
                    age_days,
                    was_fading,
                    extra_context.as_deref(),
                )
                .await
            {
                Ok(synthesis) => {
                    let action_guidance_json =
                        serde_json::to_string(&synthesis.action_guidance).unwrap_or_default();
                    if let Err(e) = self
                        .writer
                        .update_story_synthesis(
                            *story_id,
                            &synthesis.headline,
                            &synthesis.lede,
                            &synthesis.narrative,
                            &synthesis.arc.to_string(),
                            &synthesis.category.to_string(),
                            &action_guidance_json,
                        )
                        .await
                    {
                        warn!(story_id = %story_id, error = %e, "Failed to write synthesis");
                    } else {
                        // Clear synthesis_pending flag
                        let _ = self.clear_synthesis_pending(*story_id).await;
                        stats.stories_enriched += 1;
                    }
                }
                Err(e) => {
                    warn!(story_id = %story_id, error = %e, "Story synthesis LLM call failed");
                }
            }
        }
    }

    /// Phase D: Compute velocity, energy, gap_velocity for all stories; snapshot; archive zombies.
    async fn phase_velocity_energy(&self) -> Result<(), neo4rs::Error> {
        let now = Utc::now();

        // Get all stories with current metrics
        let q = query(
            "MATCH (s:Story)
             WHERE s.arc IS NULL OR s.arc <> 'archived'
             OPTIONAL MATCH (s)-[:CONTAINS]->(n)
             RETURN s.id AS id, s.source_count AS source_count,
                    s.entity_count AS entity_count, s.type_diversity AS type_diversity,
                    s.ask_count AS ask_count, s.give_count AS give_count,
                    s.last_updated AS last_updated,
                    count(n) AS signal_count",
        );

        let mut stories: Vec<(Uuid, u32, u32, u32, u32, u32, u32, String)> = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let source_count: i64 = row.get("source_count").unwrap_or(0);
            let entity_count: i64 = row.get("entity_count").unwrap_or(0);
            let type_diversity: i64 = row.get("type_diversity").unwrap_or(1);
            let signal_count: i64 = row.get("signal_count").unwrap_or(0);
            let ask_count: i64 = row.get("ask_count").unwrap_or(0);
            let give_count: i64 = row.get("give_count").unwrap_or(0);
            let last_updated: String = row.get("last_updated").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                stories.push((
                    id,
                    signal_count as u32,
                    source_count as u32,
                    entity_count as u32,
                    type_diversity as u32,
                    ask_count as u32,
                    give_count as u32,
                    last_updated,
                ));
            }
        }

        info!(stories = stories.len(), "Phase D: computing velocity and energy");

        for (story_id, current_count, source_count, entity_count, type_diversity, ask_count, give_count, last_updated_str) in stories {
            // Create snapshot
            let snapshot = rootsignal_common::ClusterSnapshot {
                id: Uuid::new_v4(),
                story_id,
                signal_count: current_count,
                entity_count,
                ask_count,
                give_count,
                run_at: now,
            };
            self.writer.create_cluster_snapshot(&snapshot).await?;

            // Velocity driven by entity diversity growth
            let entity_count_7d_ago = self
                .writer
                .get_snapshot_entity_count_7d_ago(story_id)
                .await?;
            let velocity = match entity_count_7d_ago {
                Some(old_entities) => (entity_count as f64 - old_entities as f64) / 7.0,
                None => entity_count as f64 / 7.0,
            };

            // Gap velocity
            let gap_7d_ago = self.writer.get_snapshot_gap_7d_ago(story_id).await?;
            let current_gap = ask_count as i32 - give_count as i32;
            let gap_velocity = match gap_7d_ago {
                Some(old_gap) => (current_gap as f64 - old_gap as f64) / 7.0,
                None => 0.0,
            };

            // Recency score
            let recency_score = parse_recency(&last_updated_str, &now);

            // Source diversity: min(unique_source_urls / 5.0, 1.0)
            let source_diversity = (source_count as f64 / 5.0).min(1.0);

            // Triangulation
            let triangulation = (type_diversity as f64 / 5.0).min(1.0);

            let energy = story_energy(velocity, recency_score, source_diversity, triangulation);

            // Archive zombie stories: last_updated > 30 days ago and velocity <= 0
            let age_days = {
                use chrono::NaiveDateTime;
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&last_updated_str) {
                    (now - dt.with_timezone(&Utc)).num_hours() as f64 / 24.0
                } else if let Ok(naive) =
                    NaiveDateTime::parse_from_str(&last_updated_str, "%Y-%m-%dT%H:%M:%S%.f")
                {
                    (now - naive.and_utc()).num_hours() as f64 / 24.0
                } else {
                    0.0
                }
            };
            let is_zombie = age_days > 30.0 && velocity <= 0.0;

            // Update story
            let set_clause = if is_zombie {
                "SET s.velocity = $velocity, s.energy = $energy, s.signal_count = $signal_count,
                     s.gap_velocity = $gap_velocity, s.arc = 'archived'"
            } else {
                "SET s.velocity = $velocity, s.energy = $energy, s.signal_count = $signal_count,
                     s.gap_velocity = $gap_velocity"
            };
            let q = query(&format!(
                "MATCH (s:Story {{id: $id}}) {set_clause}"
            ))
            .param("id", story_id.to_string())
            .param("velocity", velocity)
            .param("energy", energy)
            .param("signal_count", current_count as i64)
            .param("gap_velocity", gap_velocity);

            self.client.graph.run(q).await?;

            if is_zombie {
                info!(story_id = %story_id, age_days, "Archived zombie story");
            }
        }

        Ok(())
    }

    // --- Helper methods ---

    /// Count distinct signal types in a set of signal IDs.
    async fn count_type_diversity(&self, signal_ids: &[String]) -> Result<u32, neo4rs::Error> {
        let q = query(
            "UNWIND $ids AS sid
             MATCH (n {id: sid})
             WITH CASE
                 WHEN n:Gathering THEN 'gathering'
                 WHEN n:Aid THEN 'aid'
                 WHEN n:Need THEN 'need'
                 WHEN n:Notice THEN 'notice'
                 WHEN n:Tension THEN 'tension'
             END AS node_type
             RETURN count(DISTINCT node_type) AS diversity",
        )
        .param("ids", signal_ids.to_vec());

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let diversity: i64 = row.get("diversity").unwrap_or(1);
            return Ok(diversity as u32);
        }
        Ok(1)
    }

    /// Refresh story metadata from graph edges: signal_count, type_diversity, centroid,
    /// sensitivity, cause_heat, and signal type counts.
    async fn refresh_story_metadata(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(n)
             WITH s,
                  count(n) AS sig_count,
                  collect(DISTINCT n.source_url) AS urls,
                  count(DISTINCT CASE
                      WHEN n:Gathering THEN 'gathering'
                      WHEN n:Aid THEN 'aid'
                      WHEN n:Need THEN 'need'
                      WHEN n:Notice THEN 'notice'
                      WHEN n:Tension THEN 'tension'
                  END) AS type_div,
                  avg(CASE WHEN n.lat IS NOT NULL AND n.lat <> 0.0 THEN n.lat END) AS avg_lat,
                  avg(CASE WHEN n.lng IS NOT NULL AND n.lng <> 0.0 THEN n.lng END) AS avg_lng,
                  CASE
                      WHEN any(x IN collect(n.sensitivity) WHERE x = 'sensitive') THEN 'sensitive'
                      WHEN any(x IN collect(n.sensitivity) WHERE x = 'elevated') THEN 'elevated'
                      ELSE 'general'
                  END AS max_sensitivity,
                  max(CASE WHEN n:Tension THEN coalesce(n.cause_heat, 0.0) ELSE 0.0 END) AS cause_heat,
                  size([x IN collect(CASE WHEN n:Ask THEN 1 END) WHERE x IS NOT NULL]) AS ask_count,
                  size([x IN collect(CASE WHEN n:Give THEN 1 END) WHERE x IS NOT NULL]) AS give_count,
                  size([x IN collect(CASE WHEN n:Event THEN 1 END) WHERE x IS NOT NULL]) AS event_count
             SET s.signal_count = sig_count,
                 s.type_diversity = type_div,
                 s.centroid_lat = avg_lat,
                 s.centroid_lng = avg_lng,
                 s.sensitivity = max_sensitivity,
                 s.cause_heat = cause_heat,
                 s.ask_count = ask_count,
                 s.give_count = give_count,
                 s.event_count = event_count,
                 s.gap_score = ask_count - give_count,
                 s.last_updated = datetime($now)",
        )
        .param("id", story_id.to_string())
        .param("now", crate::writer::format_datetime_pub(&Utc::now()));

        self.client.graph.run(q).await?;

        // Update drawn_to_count separately (requires traversing through tension)
        let q2 = query(
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(t:Tension)<-[d:DRAWN_TO]-()
             WITH s, count(d) AS drawn_count
             SET s.drawn_to_count = drawn_count",
        )
        .param("id", story_id.to_string());
        self.client.graph.run(q2).await?;

        Ok(())
    }

    /// Check if a story was previously fading and mark pending resurgence.
    async fn check_resurgence(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})
             WHERE s.arc = 'fading'
             SET s.was_fading = true",
        )
        .param("id", story_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn set_synthesis_pending(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query("MATCH (s:Story {id: $id}) SET s.synthesis_pending = true")
            .param("id", story_id.to_string());
        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn clear_synthesis_pending(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query("MATCH (s:Story {id: $id}) SET s.synthesis_pending = false")
            .param("id", story_id.to_string());
        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn set_needs_refinement(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query("MATCH (s:Story {id: $id}) SET s.needs_refinement = true")
            .param("id", story_id.to_string());
        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get signal IDs for a story.
    async fn get_story_signal_ids(&self, story_id: Uuid) -> Result<Vec<String>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(n)
             RETURN n.id AS id",
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

    /// Fetch signal metadata for synthesis.
    async fn fetch_signal_metadata(
        &self,
        signal_ids: &[String],
    ) -> Result<Vec<SignalMeta>, neo4rs::Error> {
        let mut results = Vec::new();

        for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE n.id IN $ids
                 RETURN n.id AS id, n.title AS title, n.summary AS summary,
                        n.source_url AS source_url,
                        n.sensitivity AS sensitivity,
                        n.lat AS lat, n.lng AS lng,
                        n.cause_heat AS cause_heat"
            ))
            .param("ids", signal_ids.to_vec());

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let lat: Option<f64> = row
                    .get("lat")
                    .ok()
                    .and_then(|v: f64| if v == 0.0 { None } else { Some(v) });
                let lng: Option<f64> = row
                    .get("lng")
                    .ok()
                    .and_then(|v: f64| if v == 0.0 { None } else { Some(v) });
                results.push(SignalMeta {
                    title: row.get("title").unwrap_or_default(),
                    summary: row.get("summary").unwrap_or_default(),
                    source_url: row.get("source_url").unwrap_or_default(),
                    node_type: label.to_lowercase(),
                    lat,
                    lng,
                    sensitivity: row
                        .get("sensitivity")
                        .unwrap_or_else(|_| "general".to_string()),
                    cause_heat: row.get("cause_heat").unwrap_or(0.0),
                });
            }
        }

        Ok(results)
    }
}

/// Signal metadata for story weaving and synthesis.
struct SignalMeta {
    title: String,
    summary: String,
    source_url: String,
    node_type: String,
    lat: Option<f64>,
    lng: Option<f64>,
    sensitivity: String,
    cause_heat: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_weaver_stats_display() {
        let stats = StoryWeaverStats {
            stories_materialized: 3,
            stories_grown: 2,
            stories_enriched: 1,
            signals_linked: 8,
            stories_absorbed: 1,
            abandoned_signals: 4,
        };
        let display = format!("{stats}");
        assert!(display.contains("3 materialized"));
        assert!(display.contains("2 grown"));
        assert!(display.contains("1 enriched"));
        assert!(display.contains("8 signals linked"));
        assert!(display.contains("1 absorbed"));
        assert!(display.contains("4 abandoned"));
    }

    #[test]
    fn story_weaver_stats_default_is_zero() {
        let stats = StoryWeaverStats::default();
        assert_eq!(stats.stories_materialized, 0);
        assert_eq!(stats.stories_grown, 0);
        assert_eq!(stats.stories_enriched, 0);
        assert_eq!(stats.signals_linked, 0);
        assert_eq!(stats.stories_absorbed, 0);
        assert_eq!(stats.abandoned_signals, 0);
    }

    #[test]
    fn containment_threshold_is_half() {
        // Verifying the constant matches the design doc
        assert!((CONTAINMENT_THRESHOLD - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn mega_tension_threshold_at_30() {
        assert_eq!(MEGA_TENSION_THRESHOLD, 30);
    }

    #[test]
    fn containment_check_logic() {
        // Simulate the containment check from phase_materialize
        let hub_signals: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let story_signals: HashSet<&str> = ["a", "b", "d", "e"].iter().copied().collect();

        // 2 of 3 hub signals are in the story = 0.66 containment
        let intersection = hub_signals
            .iter()
            .filter(|id| story_signals.contains(id.as_str()))
            .count();
        let containment = intersection as f64 / hub_signals.len() as f64;
        assert!(
            containment >= CONTAINMENT_THRESHOLD,
            "Should absorb at 0.66"
        );
    }

    #[test]
    fn containment_below_threshold_creates_new_story() {
        let hub_signals: HashSet<String> =
            ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let story_signals: HashSet<&str> = ["a", "e", "f", "g"].iter().copied().collect();

        // 1 of 4 hub signals are in the story = 0.25 containment
        let intersection = hub_signals
            .iter()
            .filter(|id| story_signals.contains(id.as_str()))
            .count();
        let containment = intersection as f64 / hub_signals.len() as f64;
        assert!(
            containment < CONTAINMENT_THRESHOLD,
            "Should create new story at 0.25"
        );
    }

    #[test]
    fn editorial_context_built_for_resurgent_stories() {
        let was_fading = true;
        let velocity: f64 = 0.5;
        let age_days: f64 = 20.0;

        let mut context_parts: Vec<String> = Vec::new();

        if was_fading && velocity > 0.0 {
            let quiet_days = age_days.round() as u64;
            context_parts.push(format!(
                "This tension was quiet for approximately {quiet_days} days before new activity. \
                 This is a resurgence — note the return of attention."
            ));
        }

        assert_eq!(context_parts.len(), 1);
        assert!(context_parts[0].contains("20 days"));
        assert!(context_parts[0].contains("resurgence"));
    }

    #[test]
    fn editorial_context_built_for_multi_perspective_stories() {
        let type_set: HashSet<&str> = ["tension", "aid", "gathering"].iter().copied().collect();

        let mut context_parts: Vec<String> = Vec::new();

        if type_set.len() >= 2 {
            let has_tension = type_set.contains("tension");
            let has_response = type_set.contains("aid") || type_set.contains("gathering");
            if has_tension && has_response {
                context_parts.push(
                    "This story includes both the underlying problem AND community responses."
                        .to_string(),
                );
            }
        }

        assert_eq!(context_parts.len(), 1);
        assert!(context_parts[0].contains("problem AND community responses"));
    }

    #[test]
    fn no_editorial_context_for_single_type_stories() {
        let type_set: HashSet<&str> = ["notice"].iter().copied().collect();

        let mut context_parts: Vec<String> = Vec::new();

        if type_set.len() >= 2 {
            context_parts.push("Multiple perspectives".to_string());
        }

        assert!(context_parts.is_empty());
    }

    #[test]
    fn story_status_used_correctly_for_materialized_stories() {
        // 2 signals from 2 different domains with 2 types = confirmed
        assert_eq!(story_status(2, 2, 2), "confirmed");

        // 5 signals all same type = echo
        assert_eq!(story_status(1, 3, 5), "echo");

        // 2 signals, 1 type, 1 entity = emerging
        assert_eq!(story_status(1, 1, 2), "emerging");
    }
}
