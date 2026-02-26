use std::fmt;

use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{ActorNode, ActorType};
use rootsignal_graph::{query, GraphClient};

use crate::pipeline::traits::SignalStore;

/// Response schema for actor extraction LLM call.
#[derive(Debug, Deserialize, JsonSchema)]
struct ActorExtractionResponse {
    actors: Vec<ExtractedActor>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExtractedActor {
    /// Name of the organization, group, government body, or individual
    name: String,
    /// One of: "organization", "government_body", "coalition", "individual"
    actor_type: String,
    /// Index of the signal in the batch this actor came from (0-based)
    signal_index: usize,
}

#[derive(Debug, Default)]
pub struct ActorExtractorStats {
    pub signals_processed: usize,
    pub actors_created: usize,
    pub edges_created: usize,
}

impl fmt::Display for ActorExtractorStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Actor extractor: {} signals processed, {} actors created, {} edges created",
            self.signals_processed, self.actors_created, self.edges_created,
        )
    }
}

struct SignalInfo {
    id: Uuid,
    title: String,
    summary: String,
}

const BATCH_SIZE: usize = 8;

const SYSTEM_PROMPT: &str = r#"You are an actor extractor. Given signal summaries from a community intelligence system, extract the names of organizations, groups, government bodies, and notable individuals mentioned.

Include: nonprofits, city departments, coalitions, community groups, churches, businesses offering help, named advocacy organizations, government agencies.
Exclude: generic references like "the city", "local officials", "residents", or "the community" unless a specific named body is given. Exclude individuals unless they are public figures acting in an official capacity.

For each actor, provide:
- name: the proper name as written
- actor_type: one of "organization", "government_body", "coalition", "individual"
- signal_index: which signal (0-based) this actor was mentioned in

If a signal mentions no extractable actors, simply omit it. Return an empty actors array if none of the signals mention named actors."#;

/// Find signals with no ACTED_IN edges and extract actors from their text via LLM.
pub async fn run_actor_extraction(
    store: &dyn SignalStore,
    client: &GraphClient,
    anthropic_api_key: &str,
    region_slug: &str,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> ActorExtractorStats {
    match run_actor_extraction_inner(store, client, anthropic_api_key, region_slug, min_lat, max_lat, min_lng, max_lng).await {
        Ok(stats) => stats,
        Err(e) => {
            warn!(error = %e, "Actor extractor failed (non-fatal)");
            ActorExtractorStats::default()
        }
    }
}

async fn run_actor_extraction_inner(
    store: &dyn SignalStore,
    client: &GraphClient,
    anthropic_api_key: &str,
    _region_slug: &str,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> Result<ActorExtractorStats> {
    let mut stats = ActorExtractorStats::default();

    // Find signals with no ACTED_IN edges pointing at them, within bounding box
    let q = query(
        "MATCH (n)
         WHERE (n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension)
           AND NOT ()-[:ACTED_IN]->(n)
           AND n.lat >= $min_lat AND n.lat <= $max_lat
           AND n.lng >= $min_lng AND n.lng <= $max_lng
         RETURN n.id AS id, n.title AS title, n.summary AS summary
         ORDER BY n.extracted_at DESC
         LIMIT 200",
    )
    .param("min_lat", min_lat)
    .param("max_lat", max_lat)
    .param("min_lng", min_lng)
    .param("max_lng", max_lng);

    let mut stream = client.inner().execute(q).await?;
    let mut signals: Vec<SignalInfo> = Vec::new();
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let title: String = row.get("title").unwrap_or_default();
        let summary: String = row.get("summary").unwrap_or_default();
        if title.is_empty() && summary.is_empty() {
            continue;
        }
        signals.push(SignalInfo { id, title, summary });
    }

    if signals.is_empty() {
        info!("Actor extractor: no signals without actors found");
        return Ok(stats);
    }

    info!(
        count = signals.len(),
        "Actor extractor: found signals without actors"
    );

    let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");

    // Process in batches
    for batch in signals.chunks(BATCH_SIZE) {
        stats.signals_processed += batch.len();

        let mut user_prompt = String::from("Extract actors from these signals:\n\n");
        for (i, signal) in batch.iter().enumerate() {
            user_prompt.push_str(&format!(
                "--- Signal {} ---\nTitle: {}\nSummary: {}\n\n",
                i, signal.title, signal.summary,
            ));
        }

        let response: ActorExtractionResponse = match claude
            .extract(SYSTEM_PROMPT, &user_prompt)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Actor extraction LLM call failed, skipping batch");
                continue;
            }
        };

        for extracted in &response.actors {
            let signal = match batch.get(extracted.signal_index) {
                Some(s) => s,
                None => {
                    warn!(
                        signal_index = extracted.signal_index,
                        batch_size = batch.len(),
                        "LLM returned out-of-bounds signal_index, skipping"
                    );
                    continue;
                }
            };

            let actor_type = match extracted.actor_type.as_str() {
                "government_body" => ActorType::GovernmentBody,
                "coalition" => ActorType::Coalition,
                "individual" => ActorType::Individual,
                _ => ActorType::Organization,
            };

            // Find or create actor
            let actor_id = match store.find_actor_by_name(&extracted.name).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    let actor = ActorNode {
                        id: Uuid::new_v4(),
                        name: extracted.name.clone(),
                        actor_type,
                        entity_id: extracted.name.to_lowercase().replace(' ', "-"),
                        domains: vec![],
                        social_urls: vec![],
                        description: String::new(),
                        signal_count: 0,
                        first_seen: Utc::now(),
                        last_active: Utc::now(),
                        typical_roles: vec![],
                        bio: None,
                        location_lat: Some((min_lat + max_lat) / 2.0),
                        location_lng: Some((min_lng + max_lng) / 2.0),
                        location_name: None,
                        discovery_depth: 0,
                    };
                    if let Err(e) = store.upsert_actor(&actor).await {
                        warn!(error = %e, actor = extracted.name, "Failed to create actor");
                        continue;
                    }
                    stats.actors_created += 1;
                    actor.id
                }
                Err(e) => {
                    warn!(error = %e, actor = extracted.name, "Actor lookup failed");
                    continue;
                }
            };

            if let Err(e) = store
                .link_actor_to_signal(actor_id, signal.id, "mentioned")
                .await
            {
                warn!(error = %e, actor = extracted.name, "Failed to link actor to signal");
                continue;
            }
            stats.edges_created += 1;
        }
    }

    Ok(stats)
}
