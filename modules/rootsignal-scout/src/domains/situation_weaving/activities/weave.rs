//! Weaving activity — calls LLM and returns events instead of writing to Neo4j.

use std::sync::Arc;

use tracing::info;
use uuid::Uuid;

use ai_client::claude::Claude;
use rootsignal_common::events::SystemEvent;
use rootsignal_common::{DispatchType, ScoutScope, SensitivityLevel, SituationArc};
use rootsignal_graph::{GraphReader, WeaveCandidate, WeaveSignal};

use crate::infra::embedder::TextEmbedder;
use crate::infra::util::HAIKU_MODEL;

use super::pure;
use super::types::{SituationWeaverStats, WeavingResponse};

/// Weave a batch of signals via LLM. Returns events and updated stats.
pub async fn weave_batch(
    signals: &[WeaveSignal],
    candidates: &[WeaveCandidate],
    temp_id_map: &mut std::collections::HashMap<String, Uuid>,
    embedder: &Arc<dyn TextEmbedder>,
    anthropic_api_key: &str,
    scope: &ScoutScope,
) -> Result<(Vec<SystemEvent>, SituationWeaverStats), Box<dyn std::error::Error + Send + Sync>> {
    let mut events = Vec::new();
    let mut stats = SituationWeaverStats::default();

    // Build per-signal candidate lists
    let mut signal_candidates: Vec<serde_json::Value> = Vec::new();
    let mut all_candidate_ids: std::collections::HashSet<Uuid> =
        std::collections::HashSet::new();

    let mut sorted_signals: Vec<&WeaveSignal> = signals.iter().collect();
    sorted_signals.sort_by(|a, b| a.published_at.cmp(&b.published_at));

    let signals_json: Vec<serde_json::Value> = sorted_signals
        .iter()
        .map(|s| {
            let cands = pure::find_candidates(s, candidates);
            for (id, _, _) in &cands {
                all_candidate_ids.insert(*id);
            }
            signal_candidates.push(serde_json::json!(cands
                .iter()
                .map(|(id, n, c)| {
                    serde_json::json!({
                        "situation_id": id.to_string(),
                        "narrative_similarity": format!("{:.2}", n),
                        "causal_similarity": format!("{:.2}", c),
                    })
                })
                .collect::<Vec<_>>()));

            pure::signal_to_json(s)
        })
        .collect();

    let candidate_context: Vec<serde_json::Value> = candidates
        .iter()
        .filter(|c| all_candidate_ids.contains(&c.id))
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "headline": c.headline,
                "arc": c.arc,
                "structured_state": truncate(&c.structured_state, 500),
            })
        })
        .collect();

    let prompt = pure::build_weaving_prompt(
        &signals_json,
        &signal_candidates,
        &candidate_context,
        scope,
    );

    let claude = Claude::new(anthropic_api_key, HAIKU_MODEL);
    let response: WeavingResponse = claude.extract(pure::SYSTEM_PROMPT, &prompt).await?;

    // Process new situations first (so assignments can reference them)
    for new_sit in &response.new_situations {
        let sit_id = Uuid::new_v4();
        temp_id_map.insert(new_sit.temp_id.clone(), sit_id);

        let assigned_signal_ids: Vec<&str> = response
            .assignments
            .iter()
            .filter(|a| a.situation_id == new_sit.temp_id)
            .map(|a| a.signal_id.as_str())
            .collect();

        let (narrative_emb, centroid_lat, centroid_lng) =
            pure::compute_initial_embeddings(signals, &assigned_signal_ids);
        let causal_emb = if !new_sit.initial_structured_state.is_null() {
            let thesis = new_sit
                .initial_structured_state
                .get("root_cause_thesis")
                .and_then(|v| v.as_str())
                .unwrap_or(&new_sit.headline);
            embedder
                .embed(thesis)
                .await
                .unwrap_or(narrative_emb.clone())
        } else {
            narrative_emb.clone()
        };

        events.push(SystemEvent::SituationIdentified {
            situation_id: sit_id,
            headline: new_sit.headline.clone(),
            lede: new_sit.lede.clone(),
            arc: SituationArc::Emerging,
            temperature: 0.0,
            centroid_lat,
            centroid_lng,
            location_name: Some(new_sit.location_name.clone()).filter(|s| !s.is_empty()),
            sensitivity: SensitivityLevel::General,
            category: None,
            structured_state: serde_json::to_string(&new_sit.initial_structured_state)
                .unwrap_or_else(|_| "{}".to_string()),
            tension_heat: Some(0.0),
            clarity: Some("Fuzzy".to_string()),
            signal_count: Some(0),
            narrative_embedding: Some(narrative_emb),
            causal_embedding: Some(causal_emb),
        });
        stats.situations_created += 1;
    }

    // Process assignments
    for assignment in &response.assignments {
        let signal_id = match Uuid::parse_str(&assignment.signal_id) {
            Ok(id) => id,
            Err(_) => continue,
        };

        let situation_id = if let Some(mapped) = temp_id_map.get(&assignment.situation_id) {
            *mapped
        } else {
            match Uuid::parse_str(&assignment.situation_id) {
                Ok(id) => id,
                Err(_) => continue,
            }
        };

        let label = signals
            .iter()
            .find(|s| s.id == signal_id)
            .map(|s| s.node_type.as_str())
            .unwrap_or("Concern");

        events.push(SystemEvent::SignalAssignedToSituation {
            signal_id,
            situation_id,
            signal_label: label.to_string(),
            confidence: assignment.confidence,
            reasoning: assignment.reasoning.clone(),
        });
        stats.signals_assigned += 1;
    }

    // Process dispatches
    for dispatch_input in &response.dispatches {
        let situation_id = if let Some(mapped) = temp_id_map.get(&dispatch_input.situation_id) {
            *mapped
        } else {
            match Uuid::parse_str(&dispatch_input.situation_id) {
                Ok(id) => id,
                Err(_) => continue,
            }
        };

        let signal_ids: Vec<Uuid> = dispatch_input
            .signal_ids
            .iter()
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();

        let dispatch_type = dispatch_input
            .dispatch_type
            .parse::<DispatchType>()
            .unwrap_or(DispatchType::Update);

        events.push(SystemEvent::DispatchCreated {
            dispatch_id: Uuid::new_v4(),
            situation_id,
            body: dispatch_input.body.clone(),
            signal_ids,
            dispatch_type,
            supersedes: None,
            fidelity_score: None,
            flagged_for_review: None,
            flag_reason: None,
        });
        stats.dispatches_written += 1;
        stats.situations_updated += 1;
    }

    // Process state updates as SituationChanged events
    for update in &response.state_updates {
        let situation_id = if let Some(mapped) = temp_id_map.get(&update.situation_id) {
            *mapped
        } else {
            match Uuid::parse_str(&update.situation_id) {
                Ok(id) => id,
                Err(_) => continue,
            }
        };

        let new_state = serde_json::to_string(&update.structured_state_patch)
            .unwrap_or_else(|_| "{}".to_string());
        events.push(SystemEvent::SituationChanged {
            situation_id,
            change: rootsignal_common::events::SituationChange::StructuredState {
                old: String::new(),
                new: new_state,
            },
        });
    }

    Ok((events, stats))
}

/// Mark signals as pending for next weaving run (no LLM budget).
pub fn mark_pending(signal_ids: Vec<Uuid>, scout_run_id: &str) -> Vec<SystemEvent> {
    if signal_ids.is_empty() {
        return Vec::new();
    }
    vec![SystemEvent::SignalsPendingWeaving {
        signal_ids,
        scout_run_id: scout_run_id.to_string(),
    }]
}

/// Post-hoc verification of dispatches. Returns DispatchFlaggedForReview events.
pub async fn verify_dispatches(
    graph: &GraphReader,
) -> Result<Vec<SystemEvent>, Box<dyn std::error::Error + Send + Sync>> {
    let mut events = Vec::new();

    let dispatches_to_check = graph.unverified_dispatches(50).await?;

    for (dispatch_id, body) in &dispatches_to_check {
        // Citation check
        let cited_ids = pure::extract_signal_citations(body);
        if !cited_ids.is_empty() {
            let missing = graph.check_signal_ids_exist(&cited_ids).await?;
            if !missing.is_empty() {
                events.push(SystemEvent::DispatchFlaggedForReview {
                    dispatch_id: *dispatch_id,
                    reason: "invalid_citation".to_string(),
                });
                continue;
            }
        }

        // PII check
        if !rootsignal_common::detect_pii(body).is_empty() {
            events.push(SystemEvent::DispatchFlaggedForReview {
                dispatch_id: *dispatch_id,
                reason: "pii_detected".to_string(),
            });
            continue;
        }

        // Citation coverage
        if pure::has_uncited_factual_claims(body) {
            events.push(SystemEvent::DispatchFlaggedForReview {
                dispatch_id: *dispatch_id,
                reason: "uncited_claim".to_string(),
            });
            continue;
        }
    }

    Ok(events)
}

fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..s
            .char_indices()
            .take(max_len)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(max_len)]
    }
}
