use std::fmt;

use ai_client::{ai_extract, Agent};
use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::ActorType;
use rootsignal_graph::{GraphQueries, UnlinkedSignal};

use crate::traits::SignalReader;

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

pub struct ActorExtractionResult {
    pub stats: ActorExtractorStats,
    pub new_actors: Vec<NewActor>,
    pub actor_links: Vec<ActorLink>,
}

pub struct NewActor {
    pub actor_id: Uuid,
    pub name: String,
    pub actor_type: ActorType,
    pub canonical_key: String,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
}

pub struct ActorLink {
    pub actor_id: Uuid,
    pub signal_id: Uuid,
    pub role: String,
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
    store: &dyn SignalReader,
    graph: &dyn GraphQueries,
    ai: &dyn Agent,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> ActorExtractionResult {
    match try_extract_actors(
        store,
        graph,
        ai,
        min_lat,
        max_lat,
        min_lng,
        max_lng,
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            warn!(error = %e, "Actor extractor failed (non-fatal)");
            ActorExtractionResult {
                stats: ActorExtractorStats::default(),
                new_actors: Vec::new(),
                actor_links: Vec::new(),
            }
        }
    }
}

async fn try_extract_actors(
    store: &dyn SignalReader,
    graph: &dyn GraphQueries,
    ai: &dyn Agent,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> Result<ActorExtractionResult> {
    let mut stats = ActorExtractorStats::default();
    let mut new_actors = Vec::new();
    let mut actor_links = Vec::new();

    let signals_raw = graph.find_signals_without_actors(min_lat, max_lat, min_lng, max_lng).await?;
    let signals: Vec<SignalInfo> = signals_raw.into_iter().map(|s| SignalInfo { id: s.id, title: s.title, summary: s.summary }).collect();

    if signals.is_empty() {
        info!("Actor extractor: no signals without actors found");
        return Ok(ActorExtractionResult { stats, new_actors, actor_links });
    }

    info!(
        count = signals.len(),
        "Actor extractor: found signals without actors"
    );

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

        let response: ActorExtractionResponse =
            match ai_extract(ai, SYSTEM_PROMPT, &user_prompt).await {
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
                    let new_id = Uuid::new_v4();
                    new_actors.push(NewActor {
                        actor_id: new_id,
                        name: extracted.name.clone(),
                        actor_type,
                        canonical_key: extracted.name.to_lowercase().replace(' ', "-"),
                        location_lat: Some((min_lat + max_lat) / 2.0),
                        location_lng: Some((min_lng + max_lng) / 2.0),
                    });
                    stats.actors_created += 1;
                    new_id
                }
                Err(e) => {
                    warn!(error = %e, actor = extracted.name, "Actor lookup failed");
                    continue;
                }
            };

            actor_links.push(ActorLink {
                actor_id,
                signal_id: signal.id,
                role: "mentioned".into(),
            });
            stats.edges_created += 1;
        }
    }

    Ok(ActorExtractionResult { stats, new_actors, actor_links })
}
