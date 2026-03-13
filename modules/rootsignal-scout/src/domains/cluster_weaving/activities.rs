use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use ai_client::{ai_extract, Agent};
use rootsignal_common::events::SystemEvent;
use rootsignal_common::{DispatchType, SensitivityLevel, SituationArc};
use rootsignal_graph::{GraphQueries, WeaveSignal};

use crate::core::engine::ScoutEngineDeps;
use crate::infra::embedder::TextEmbedder;

#[derive(Debug, Deserialize, JsonSchema)]
struct ClusterNarrative {
    headline: String,
    lede: String,
    briefing_body: String,
    structured_state: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeltaDispatch {
    body: String,
}

const SYSTEM_PROMPT: &str = "\
You are writing a local situation briefing. \
Write in a warm, direct, action-oriented tone. Third person only — never use \"we\", \
\"our\", \"us\", or \"together\". Report what is happening, who is involved, and what \
is needed. No bureaucratic language.";

fn build_first_weave_prompt(label: &str, signals: &[WeaveSignal]) -> String {
    let signal_list: Vec<String> = signals
        .iter()
        .map(|s| format!("- [{}] {}: {}", s.node_type, s.title, truncate(&s.summary, 200)))
        .collect();

    format!(
        "Cluster label: {label}\n\n\
         Member signals ({count}):\n{signals}\n\n\
         Produce JSON with:\n\
         - \"headline\": one sentence capturing the situation\n\
         - \"lede\": 2-3 sentences of context\n\
         - \"briefing_body\": a 3-5 paragraph narrative in markdown covering:\n\
           1. What's happening (the core situation)\n\
           2. How people are responding (who's organizing, what's underway)\n\
           3. What's needed (explicit asks — e.g. \"volunteers needed for X\")\n\
           Be warm, direct, action-oriented. Third person only — no \"we\" or \"our\".\n\
           Use **bold** for key details.\n\
         - \"structured_state\": {{\"root_cause_thesis\": \"...\", \"key_actors\": [...], \"status\": \"emerging\"}}",
        count = signals.len(),
        signals = signal_list.join("\n"),
    )
}

fn build_delta_dispatch_prompt(label: &str, new_signals: &[WeaveSignal]) -> String {
    let signal_list: Vec<String> = new_signals
        .iter()
        .map(|s| format!("- [{}] {}: {}", s.node_type, s.title, truncate(&s.summary, 200)))
        .collect();

    format!(
        "Cluster: {label}\n\n\
         New signals added since last weave ({count}):\n{signals}\n\n\
         Write a brief dispatch (2-3 sentences) summarizing the update. \
         Return JSON with a single \"body\" field.",
        count = new_signals.len(),
        signals = signal_list.join("\n"),
    )
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Run cluster weaving: read a SignalGroup from Neo4j, produce a Situation.
pub async fn weave_cluster(deps: &ScoutEngineDeps, group_id: Uuid) -> causal::Events {
    let mut events = causal::Events::new();

    let (graph, ai) = match (deps.graph.as_deref(), deps.ai.as_deref()) {
        (Some(g), Some(a)) => (g, a),
        _ => {
            warn!("ClusterWeaver: missing graph or AI deps");
            return events;
        }
    };

    let detail = match graph.get_cluster_detail(group_id).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            warn!(%group_id, "ClusterWeaver: group not found");
            return events;
        }
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: failed to read cluster");
            return events;
        }
    };

    let label = &detail.label;

    if let Some(situation_id) = detail.woven_situation_id {
        reweave(graph, ai, &deps.embedder, group_id, situation_id, label, &mut events).await;
    } else {
        first_weave(graph, ai, &deps.embedder, group_id, label, &mut events).await;
    }

    events
}

async fn first_weave(
    graph: &dyn GraphQueries,
    ai: &dyn Agent,
    embedder: &Arc<dyn TextEmbedder>,
    group_id: Uuid,
    label: &str,
    events: &mut causal::Events,
) {
    let signals = match graph.get_cluster_members(group_id).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: failed to read members");
            return;
        }
    };

    if signals.is_empty() {
        info!(%group_id, "ClusterWeaver: no member signals, skipping");
        return;
    }

    let prompt = build_first_weave_prompt(label, &signals);
    let narrative: ClusterNarrative = match ai_extract(ai, SYSTEM_PROMPT, &prompt).await {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: LLM call failed");
            return;
        }
    };

    let situation_id = Uuid::new_v4();

    let (narrative_emb, centroid_lat, centroid_lng) = compute_centroid(&signals, embedder).await;
    let causal_emb = narrative
        .structured_state
        .get("root_cause_thesis")
        .and_then(|v| v.as_str())
        .map(|thesis| embedder.embed(thesis));
    let causal_embedding = match causal_emb {
        Some(fut) => fut.await.ok().or(Some(narrative_emb.clone())),
        None => Some(narrative_emb.clone()),
    };

    events.push(SystemEvent::SituationIdentified {
        situation_id,
        headline: narrative.headline,
        lede: narrative.lede,
        arc: SituationArc::Emerging,
        temperature: 0.0,
        centroid_lat,
        centroid_lng,
        location_name: None,
        sensitivity: SensitivityLevel::General,
        category: None,
        structured_state: serde_json::to_string(&narrative.structured_state)
            .unwrap_or_else(|_| "{}".to_string()),
        tension_heat: Some(0.0),
        clarity: Some("Fuzzy".to_string()),
        signal_count: Some(signals.len() as u32),
        narrative_embedding: Some(narrative_emb),
        causal_embedding,
        briefing_body: Some(narrative.briefing_body),
    });

    events.push(SystemEvent::GroupWovenIntoSituation {
        group_id,
        situation_id,
    });

    info!(%group_id, %situation_id, signals = signals.len(), "ClusterWeaver: first weave complete");
}

async fn reweave(
    graph: &dyn GraphQueries,
    ai: &dyn Agent,
    _embedder: &Arc<dyn TextEmbedder>,
    group_id: Uuid,
    situation_id: Uuid,
    label: &str,
    events: &mut causal::Events,
) {
    // Fetch ALL members to regenerate the full briefing
    let all_signals = match graph.get_cluster_members(group_id).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: failed to read members for re-weave");
            return;
        }
    };

    if all_signals.is_empty() {
        info!(%group_id, "ClusterWeaver: no member signals, skipping re-weave");
        return;
    }

    // Regenerate full briefing from all signals
    let prompt = build_first_weave_prompt(label, &all_signals);
    let narrative: ClusterNarrative = match ai_extract(ai, SYSTEM_PROMPT, &prompt).await {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: re-weave LLM call failed");
            return;
        }
    };

    events.push(SystemEvent::SituationChanged {
        situation_id,
        change: rootsignal_common::events::SituationChange::BriefingBody {
            new: narrative.briefing_body,
        },
    });

    events.push(SystemEvent::SituationChanged {
        situation_id,
        change: rootsignal_common::events::SituationChange::Headline {
            old: String::new(),
            new: narrative.headline,
        },
    });

    events.push(SystemEvent::SituationChanged {
        situation_id,
        change: rootsignal_common::events::SituationChange::Lede {
            old: String::new(),
            new: narrative.lede,
        },
    });

    if !narrative.structured_state.is_null() {
        let new_state = serde_json::to_string(&narrative.structured_state)
            .unwrap_or_else(|_| "{}".to_string());
        events.push(SystemEvent::SituationChanged {
            situation_id,
            change: rootsignal_common::events::SituationChange::StructuredState {
                old: String::new(),
                new: new_state,
            },
        });
    }

    // Also produce a delta dispatch if there are new signals since last weave
    let delta = match graph.get_cluster_delta_signals(group_id).await {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: failed to read delta signals (non-fatal)");
            Vec::new()
        }
    };

    if !delta.is_empty() {
        let dispatch_prompt = build_delta_dispatch_prompt(label, &delta);
        if let Ok(dispatch) = ai_extract::<DeltaDispatch>(ai, SYSTEM_PROMPT, &dispatch_prompt).await {
            let signal_ids: Vec<Uuid> = delta.iter().map(|s| s.id).collect();
            events.push(SystemEvent::DispatchCreated {
                dispatch_id: Uuid::new_v4(),
                situation_id,
                body: dispatch.body,
                signal_ids,
                dispatch_type: DispatchType::Update,
                supersedes: None,
                fidelity_score: None,
                flagged_for_review: None,
                flag_reason: None,
            });
        }
    }

    // Update woven_at timestamp
    events.push(SystemEvent::GroupWovenIntoSituation {
        group_id,
        situation_id,
    });

    // Recompute temperature
    match graph.compute_situation_temperature(&situation_id).await {
        Ok((components, temp_events)) => {
            info!(
                %situation_id,
                temperature = components.temperature,
                arc = %components.arc,
                "ClusterWeaver: temperature recomputed"
            );
            for ev in temp_events {
                events.push(ev);
            }
        }
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: temperature recomputation failed");
        }
    }

    info!(%group_id, %situation_id, signals = all_signals.len(), "ClusterWeaver: re-weave complete");
}

async fn compute_centroid(
    signals: &[WeaveSignal],
    embedder: &Arc<dyn TextEmbedder>,
) -> (Vec<f32>, Option<f64>, Option<f64>) {
    let mut lat_sum = 0.0f64;
    let mut lng_sum = 0.0f64;
    let mut geo_count = 0u32;

    for s in signals {
        if let (Some(lat), Some(lng)) = (s.lat, s.lng) {
            lat_sum += lat;
            lng_sum += lng;
            geo_count += 1;
        }
    }

    let centroid_lat = if geo_count > 0 { Some(lat_sum / geo_count as f64) } else { None };
    let centroid_lng = if geo_count > 0 { Some(lng_sum / geo_count as f64) } else { None };

    let combined = signals
        .iter()
        .map(|s| format!("{}: {}", s.title, truncate(&s.summary, 100)))
        .collect::<Vec<_>>()
        .join("\n");

    let narrative_emb = match embedder.embed(&combined).await {
        Ok(emb) => emb,
        Err(e) => {
            warn!(error = %e, "ClusterWeaver: narrative embedding failed, using empty vector");
            Vec::new()
        }
    };

    (narrative_emb, centroid_lat, centroid_lng)
}
