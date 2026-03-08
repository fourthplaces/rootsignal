//! Shared utilities for synthesis finders (response_finder, gathering_finder).

use anyhow::Result;
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use rootsignal_common::{
    canonical_value, DiscoveryMethod, GeoPoint, GeoPrecision, NodeMeta, ReviewStatus, ScoutScope,
    SensitivityLevel, SourceNode, SourceRole,
};
use rootsignal_graph::GraphReader;

use crate::domains::discovery::activities::source_finder::initial_weight_for_method;
use crate::infra::embedder::TextEmbedder;

pub use crate::infra::util::HAIKU_MODEL;
pub const MAX_TOOL_TURNS: usize = 10;
pub const MAX_FUTURE_QUERIES_PER_TENSION: usize = 3;

/// Build a future query SourceNode for a finder.
/// `finder_name` is used in the gap_context, e.g. "Response finder" or "Gathering finder".
pub fn build_future_query_source(
    query: &str,
    tension_title: &str,
    finder_name: &str,
) -> SourceNode {
    let cv = query.to_string();
    let ck = canonical_value(&cv);
    let gap_context = format!(
        "{finder_name}: gathering discovery for \"{tension_title}\"",
    );

    info!(
        query = query,
        tension = tension_title,
        "Future query source built by {finder_name}",
    );

    SourceNode {
        id: Uuid::new_v4(),
        canonical_key: ck,
        canonical_value: cv,
        url: None,
        discovery_method: DiscoveryMethod::GapAnalysis,
        created_at: Utc::now(),
        last_scraped: None,
        last_produced_signal: None,
        signals_produced: 0,
        signals_corroborated: 0,
        consecutive_empty_runs: 0,
        active: true,
        gap_context: Some(gap_context),
        weight: initial_weight_for_method(DiscoveryMethod::GapAnalysis, Some("unmet_tension")),
        cadence_hours: None,
        avg_signals_per_scrape: 0.0,
        quality_penalty: 1.0,
        source_role: SourceRole::Response,
        scrape_count: 0,
        sources_discovered: 0,
        discovered_from_key: None,
        channel_weights: rootsignal_common::ChannelWeights::default(),
    }
}

/// Build a NodeMeta with region-center coordinates and sensible defaults.
pub fn build_node_meta(
    title: String,
    summary: String,
    source_url: String,
    region: &ScoutScope,
    confidence: f32,
) -> NodeMeta {
    let now = Utc::now();
    NodeMeta {
        id: Uuid::new_v4(),
        title,
        summary,
        sensitivity: SensitivityLevel::General,
        confidence,
        corroboration_count: 0,
        about_location: Some(GeoPoint {
            lat: region.center_lat,
            lng: region.center_lng,
            precision: GeoPrecision::Approximate,
        }),
        from_location: None,
        about_location_name: Some(region.name.clone()),
        source_url,
        extracted_at: now,
        published_at: None,
        last_confirmed_active: now,
        source_diversity: 1,
        cause_heat: 0.0,
        channel_diversity: 1,
        implied_queries: vec![],
        review_status: ReviewStatus::Staged,
        was_corrected: false,
        corrections: None,
        rejection_reason: None,
        mentioned_actors: Vec::new(),
        category: None,
    }
}

/// Compute bounding box from a region scope.
pub fn region_bounds(region: &ScoutScope) -> (f64, f64, f64, f64) {
    let lat_delta = region.radius_km / 111.0;
    let lng_delta = region.radius_km / (111.0 * region.center_lat.to_radians().cos());
    (
        region.center_lat - lat_delta,
        region.center_lat + lat_delta,
        region.center_lng - lng_delta,
        region.center_lng + lng_delta,
    )
}

/// Find the best-matching active tension by embedding similarity.
/// Returns (concern_id, similarity) if above the threshold.
pub async fn find_best_tension_match(
    embedder: &dyn TextEmbedder,
    graph: &GraphReader,
    region: &ScoutScope,
    tension_title: &str,
    threshold: f64,
) -> Result<Option<(Uuid, f64)>> {
    let (min_lat, max_lat, min_lng, max_lng) = region_bounds(region);
    let active_tensions = graph
        .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
        .await?;
    if active_tensions.is_empty() {
        return Ok(None);
    }

    let title_embedding = embedder.embed(tension_title).await?;
    let title_emb_f64: Vec<f64> = title_embedding.iter().map(|&v| v as f64).collect();

    let mut best: Option<(Uuid, f64)> = None;
    for (tid, temb) in &active_tensions {
        let sim = crate::infra::util::cosine_similarity(&title_emb_f64, temb);
        if sim >= threshold {
            if best.as_ref().map_or(true, |b| sim > b.1) {
                best = Some((*tid, sim));
            }
        }
    }

    Ok(best)
}
