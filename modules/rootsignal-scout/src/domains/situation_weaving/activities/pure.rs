//! Pure functions for situation weaving — no I/O, no side effects.

use uuid::Uuid;

use rootsignal_common::ScoutScope;
use rootsignal_graph::{WeaveCandidate, WeaveSignal};

const NARRATIVE_SIMILARITY_THRESHOLD: f64 = 0.6;
const WIDE_NET_THRESHOLD: f64 = 0.45;
const WIDE_NET_HEAT_MIN: f64 = 0.5;
const COLD_REACTIVATION_NARRATIVE: f64 = 0.75;
const COLD_REACTIVATION_CAUSAL: f64 = 0.80;
const TOP_K_CANDIDATES: usize = 5;

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += (*x as f64) * (*y as f64);
        norm_a += (*x as f64) * (*x as f64);
        norm_b += (*y as f64) * (*y as f64);
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Find top candidate situations for a signal based on embedding similarity.
pub fn find_candidates(
    signal: &WeaveSignal,
    candidates: &[WeaveCandidate],
) -> Vec<(Uuid, f64, f64)> {
    let mut scored: Vec<(Uuid, f64, f64)> = candidates
        .iter()
        .filter(|c| !c.narrative_embedding.is_empty())
        .map(|c| {
            let narrative_sim = cosine_similarity(&signal.embedding, &c.narrative_embedding);
            let causal_sim = cosine_similarity(&signal.embedding, &c.causal_embedding);
            (c.id, narrative_sim, causal_sim)
        })
        .collect();

    scored.sort_by(|a, b| {
        let max_a = a.1.max(a.2);
        let max_b = b.1.max(b.2);
        max_b
            .partial_cmp(&max_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut results: Vec<(Uuid, f64, f64)> = scored
        .iter()
        .filter(|(id, narrative_sim, causal_sim)| {
            let is_cold = candidates
                .iter()
                .find(|c| c.id == *id)
                .map(|c| c.arc == "cold")
                .unwrap_or(false);

            if is_cold {
                *narrative_sim >= COLD_REACTIVATION_NARRATIVE
                    || *causal_sim >= COLD_REACTIVATION_CAUSAL
            } else {
                *narrative_sim >= NARRATIVE_SIMILARITY_THRESHOLD
                    || *causal_sim >= NARRATIVE_SIMILARITY_THRESHOLD
            }
        })
        .take(TOP_K_CANDIDATES)
        .copied()
        .collect();

    // Wide Net fallback: high cause_heat signal with low similarity
    if results.is_empty()
        && signal.cause_heat >= WIDE_NET_HEAT_MIN
        && scored.first().map(|s| s.1.max(s.2)).unwrap_or(0.0) < NARRATIVE_SIMILARITY_THRESHOLD
    {
        let developing_matches: Vec<(Uuid, f64, f64)> = scored
            .iter()
            .filter(|(id, narrative_sim, causal_sim)| {
                let is_developing = candidates
                    .iter()
                    .find(|c| c.id == *id)
                    .map(|c| c.arc == "developing")
                    .unwrap_or(false);
                is_developing
                    && (*narrative_sim >= WIDE_NET_THRESHOLD || *causal_sim >= WIDE_NET_THRESHOLD)
            })
            .take(TOP_K_CANDIDATES)
            .copied()
            .collect();

        if !developing_matches.is_empty() {
            results = developing_matches;
        }
    }

    results
}

/// Compute initial narrative embedding as mean of assigned signal embeddings.
/// Also returns centroid lat/lng.
pub fn compute_initial_embeddings(
    signals: &[WeaveSignal],
    assigned_ids: &[&str],
) -> (Vec<f32>, Option<f64>, Option<f64>) {
    let assigned: Vec<&WeaveSignal> = signals
        .iter()
        .filter(|s| assigned_ids.contains(&s.id.to_string().as_str()))
        .collect();

    if assigned.is_empty() {
        return (vec![0.0; 1024], None, None);
    }

    let dim = assigned[0].embedding.len();
    let mut centroid = vec![0.0f64; dim];
    for s in &assigned {
        for (i, v) in s.embedding.iter().enumerate() {
            if i < dim {
                centroid[i] += *v as f64;
            }
        }
    }
    let n = assigned.len() as f64;
    let embedding: Vec<f32> = centroid.iter().map(|v| (*v / n) as f32).collect();

    let lats: Vec<f64> = assigned.iter().filter_map(|s| s.lat).collect();
    let lngs: Vec<f64> = assigned.iter().filter_map(|s| s.lng).collect();
    let centroid_lat = if lats.is_empty() {
        None
    } else {
        Some(lats.iter().sum::<f64>() / lats.len() as f64)
    };
    let centroid_lng = if lngs.is_empty() {
        None
    } else {
        Some(lngs.iter().sum::<f64>() / lngs.len() as f64)
    };

    (embedding, centroid_lat, centroid_lng)
}

/// Build the weaving prompt for the LLM.
pub fn build_weaving_prompt(
    signals: &[serde_json::Value],
    signal_candidates: &[serde_json::Value],
    candidate_situations: &[serde_json::Value],
    scope: &ScoutScope,
) -> String {
    let signals_with_candidates: Vec<serde_json::Value> = signals
        .iter()
        .zip(signal_candidates.iter())
        .map(|(s, c)| {
            let mut obj = s.clone();
            obj.as_object_mut()
                .unwrap()
                .insert("candidate_situations".to_string(), c.clone());
            obj
        })
        .collect();

    format!(
        r#"Assign these new signals to situations and write dispatches.

NEW SIGNALS:
{}

CANDIDATE SITUATIONS:
{}

SCOPE: {} (lat: {}, lng: {})

For each signal:
- If it matches an existing situation (same root cause + place), assign it
- If no situation matches, create a new one with temp_id "NEW-1", "NEW-2", etc.
- Write a dispatch for each affected situation summarizing the new information
- Update structured_state if the thesis, confidence, or timeline changes

Return JSON with: assignments, new_situations, dispatches, state_updates"#,
        serde_json::to_string_pretty(&signals_with_candidates).unwrap_or_default(),
        serde_json::to_string_pretty(&candidate_situations).unwrap_or_default(),
        scope.name,
        scope.center_lat,
        scope.center_lng,
    )
}

/// Format a signal as JSON for the LLM prompt.
pub fn signal_to_json(s: &WeaveSignal) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "id": s.id.to_string(),
        "title": s.title,
        "summary": truncate(&s.summary, 300),
        "type": s.node_type,
        "cause_heat": format!("{:.2}", s.cause_heat),
        "source_url": s.source_url,
        "location": format_location(s.lat, s.lng),
    });
    if let Some(date) = &s.published_at {
        obj.as_object_mut()
            .unwrap()
            .insert("published".to_string(), serde_json::json!(date));
    }
    obj
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

fn format_location(lat: Option<f64>, lng: Option<f64>) -> String {
    match (lat, lng) {
        (Some(lat), Some(lng)) => format!("{:.4}, {:.4}", lat, lng),
        _ => "unknown".to_string(),
    }
}

/// Extract [signal:UUID] citations from dispatch text.
pub fn extract_signal_citations(body: &str) -> Vec<Uuid> {
    let mut ids = Vec::new();
    let marker = "[signal:";
    let mut search = body;
    while let Some(start) = search.find(marker) {
        let after = &search[start + marker.len()..];
        if let Some(end) = after.find(']') {
            if let Ok(id) = Uuid::parse_str(&after[..end]) {
                ids.push(id);
            }
            search = &after[end..];
        } else {
            break;
        }
    }
    ids
}

/// Check if dispatch body contains factual claims without signal citations.
pub fn has_uncited_factual_claims(body: &str) -> bool {
    let sentences: Vec<&str> = body
        .split(|c: char| c == '.' || c == '!' || c == '?')
        .filter(|s| !s.trim().is_empty())
        .collect();

    for sentence in &sentences {
        let trimmed = sentence.trim();
        if trimmed.split_whitespace().count() < 4 {
            continue;
        }
        if trimmed.contains("[signal:") {
            continue;
        }
        let has_number = trimmed.chars().any(|c| c.is_ascii_digit());
        let has_proper_noun = trimmed
            .split_whitespace()
            .skip(1)
            .any(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false));
        if has_number || has_proper_noun {
            return true;
        }
    }
    false
}

pub const SYSTEM_PROMPT: &str = r#"You are a situation tracker for a civic intelligence system. Your job is to assign signals to situations and write factual dispatches.

HARD RULES:
1. Every claim in a dispatch MUST cite a specific signal by ID using [signal:UUID] format.
2. If sources disagree on cause, present ALL claims side by side. Never pick a winner.
3. Describe what is happening, not what it means. "3 new eviction filings" not "the crisis deepens."
4. Use invitational, factual tone. Urgency is about opportunity windows, not threats.
5. If a signal doesn't fit any candidate situation, create a new one.
6. Do not infer geographic parallels. Only associate a signal with a situation if the source EXPLICITLY references that place/issue.
7. Different root cause = different situation, even if same place and same surface effect.
8. Do NOT assign actor roles (decision-maker, beneficiary, etc). Only list actor NAMES that appear in signals. Role analysis requires a separate, higher-accuracy process.
9. If new evidence contradicts an existing dispatch, write a "correction" dispatch that references what it supersedes. Do not silently change the structured state.
10. Actively challenge the existing root_cause_thesis when new evidence suggests alternatives. Do not confirm the thesis by default.
11. SEMANTIC FRICTION: If two signals are geographically close but semantically distant, you MUST explain why they belong to the SAME situation. Default to separate situations when geography overlaps but content diverges.
12. LEAD WITH RESPONSES: When writing dispatches about situations that have both tensions AND responses, lead with the response. The response is the primary signal; the tension provides context.
13. Signals are listed in chronological order by publication date. Respect this ordering in dispatches.

Respond with valid JSON matching the WeavingResponse schema."#;
