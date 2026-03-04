//! Maps responses (Aid/Gathering) to active Tensions/Needs using embedding
//! similarity + LLM verification.
//!
//! Moved from `rootsignal-graph::response` — this is discovery logic (query → LLM
//! verify → write), not a graph primitive. Follows the same pattern as the other
//! finders: `&GraphReader` for reads, engine dispatch for writes.

use ai_client::{ai_extract, Agent};
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};

use rootsignal_common::system_events::SystemEvent;
use rootsignal_common::telemetry_events::TelemetryEvent;
use rootsignal_graph::GraphReader;

use seesaw_core::Events;

/// Structured output for response verification.
#[derive(Deserialize, JsonSchema)]
struct ResponseVerdict {
    /// Whether signal B responds to problem A
    matches: bool,
    /// Brief explanation of how B helps address A (only when matches is true)
    explanation: Option<String>,
}

/// For each active Tension/Need, find Aid/Gathering signals that might respond to it.
/// Uses embedding similarity as a cheap filter, then LLM as a verifier.
pub async fn map_responses(
    graph: &GraphReader,
    ai: &dyn Agent,
    center_lat: f64,
    center_lng: f64,
    radius_km: f64,
    events: &mut Events,
) -> Result<ResponseMappingStats, Box<dyn std::error::Error + Send + Sync>> {
    let lat_delta = radius_km / 111.0;
    let lng_delta = radius_km / (111.0 * center_lat.to_radians().cos());
    let min_lat = center_lat - lat_delta;
    let max_lat = center_lat + lat_delta;
    let min_lng = center_lng - lng_delta;
    let max_lng = center_lng + lng_delta;

    let mut stats = ResponseMappingStats::default();

    let tensions = graph
        .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
        .await?;
    if tensions.is_empty() {
        info!("No active tensions for response mapping");
        return Ok(stats);
    }

    info!(tensions = tensions.len(), "Running response mapping");

    for (concern_id, tension_embedding) in &tensions {
        let candidates = graph
            .find_response_candidates(
                tension_embedding,
                min_lat,
                max_lat,
                min_lng,
                max_lng,
            )
            .await?;
        stats.candidates_found += candidates.len() as u32;

        if candidates.is_empty() {
            continue;
        }

        let tension_info = graph.get_signal_info(*concern_id).await?;
        let Some((tension_title, tension_summary)) = tension_info else {
            continue;
        };

        let mut verified = 0u32;
        let checked = candidates.len().min(5) as u32;
        for (candidate_id, candidate_similarity) in candidates.iter().take(5) {
            let candidate_info = graph.get_signal_info(*candidate_id).await?;
            let Some((candidate_title, candidate_summary)) = candidate_info else {
                continue;
            };

            match verify_response(
                ai,
                &tension_title,
                &tension_summary,
                &candidate_title,
                &candidate_summary,
            )
            .await
            {
                Ok(Some(explanation)) => {
                    events.push(SystemEvent::ResponseLinked {
                        signal_id: *candidate_id,
                        concern_id: *concern_id,
                        strength: *candidate_similarity,
                        explanation: explanation.clone(),
                        source_url: None,
                    });
                    stats.edges_created += 1;
                    verified += 1;
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(error = %e, "LLM verification failed");
                }
            }
        }

        events.push(TelemetryEvent::SystemLog {
            message: format!(
                "LLM response mapping: \"{}\" — {}/{} candidates matched",
                tension_title, verified, checked,
            ),
            context: Some(serde_json::json!({
                "activity": "response_mapper",
                "concern_id": concern_id.to_string(),
                "candidates_checked": checked,
                "candidates_matched": verified,
            })),
        });
    }

    info!(
        edges = stats.edges_created,
        candidates = stats.candidates_found,
        "Response mapping complete"
    );
    Ok(stats)
}

/// Map responses for a single tension — returns events and edge count.
/// Used by the per-target handler.
pub async fn map_single_tension(
    graph: &GraphReader,
    ai: &dyn Agent,
    concern_id: uuid::Uuid,
    tension_embedding: &[f64],
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> (Events, u32) {
    let mut events = Events::new();
    let mut edges_created = 0u32;

    let candidates = match graph
        .find_response_candidates(tension_embedding, min_lat, max_lat, min_lng, max_lng)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to find response candidates for tension");
            return (events, edges_created);
        }
    };

    if candidates.is_empty() {
        return (events, edges_created);
    }

    let tension_info = match graph.get_signal_info(concern_id).await {
        Ok(Some((title, summary))) => (title, summary),
        Ok(None) => return (events, edges_created),
        Err(e) => {
            warn!(error = %e, "Failed to get tension info");
            return (events, edges_created);
        }
    };

    let (tension_title, tension_summary) = tension_info;
    let mut verified = 0u32;
    let checked = candidates.len().min(5) as u32;

    for (candidate_id, candidate_similarity) in candidates.iter().take(5) {
        let candidate_info = match graph.get_signal_info(*candidate_id).await {
            Ok(Some(info)) => info,
            _ => continue,
        };
        let (candidate_title, candidate_summary) = candidate_info;

        match verify_response(
            ai,
            &tension_title,
            &tension_summary,
            &candidate_title,
            &candidate_summary,
        )
        .await
        {
            Ok(Some(explanation)) => {
                events.push(SystemEvent::ResponseLinked {
                    signal_id: *candidate_id,
                    concern_id,
                    strength: *candidate_similarity,
                    explanation: explanation.clone(),
                    source_url: None,
                });
                edges_created += 1;
                verified += 1;
            }
            Ok(None) => {}
            Err(e) => {
                warn!(error = %e, "LLM verification failed");
            }
        }
    }

    events.push(TelemetryEvent::SystemLog {
        message: format!(
            "LLM response mapping: \"{}\" — {}/{} candidates matched",
            tension_title, verified, checked,
        ),
        context: Some(serde_json::json!({
            "activity": "response_mapper",
            "concern_id": concern_id.to_string(),
            "candidates_checked": checked,
            "candidates_matched": verified,
        })),
    });

    (events, edges_created)
}

/// LLM verifies whether a candidate signal actually responds to a tension.
async fn verify_response(
    ai: &dyn Agent,
    tension_title: &str,
    tension_summary: &str,
    candidate_title: &str,
    candidate_summary: &str,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let prompt = format!(
        r#"Does Signal B respond to or help address Problem A?

Problem A: {tension_title} — {tension_summary}
Signal B: {candidate_title} — {candidate_summary}

Determine whether B genuinely helps address A. Be strict — only confirm genuine matches."#,
    );

    let verdict = ai_extract::<ResponseVerdict>(
        ai,
        "You evaluate whether community resources respond to community needs. Be strict — only confirm genuine matches.",
        &prompt,
    )
    .await?;

    if verdict.matches {
        Ok(verdict.explanation)
    } else {
        Ok(None)
    }
}

#[derive(Debug, Default)]
pub struct ResponseMappingStats {
    pub candidates_found: u32,
    pub edges_created: u32,
}

impl std::fmt::Display for ResponseMappingStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Response Mapping ===")?;
        writeln!(f, "Candidates found: {}", self.candidates_found)?;
        writeln!(f, "Edges created:    {}", self.edges_created)?;
        Ok(())
    }
}
