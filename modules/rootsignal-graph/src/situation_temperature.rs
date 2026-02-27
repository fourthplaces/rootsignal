//! Temperature + Clarity computation for Situations.
//!
//! Temperature derives entirely from graph mechanics — no LLM dependency.
//!
//! Formula:
//!   substance = min(tension_heat_agg + entity_velocity_norm, 1.0)
//!   amplification_contrib = amplification_norm * substance
//!   temperature = 0.30 * tension_heat_agg
//!              + 0.25 * entity_velocity_norm
//!              + 0.15 * response_gap_norm
//!              + 0.15 * amplification_contrib
//!              + 0.15 * clarity_need_norm

use chrono::{DateTime, Duration, Utc};
use neo4rs::query;
use uuid::Uuid;

use rootsignal_common::{Clarity, SituationArc};

use crate::writer::GraphWriter;
use crate::GraphClient;

/// All computed temperature components for a situation.
#[derive(Debug, Clone)]
pub struct TemperatureComponents {
    pub tension_heat_agg: f64,
    pub entity_velocity_norm: f64,
    pub response_gap_norm: f64,
    pub amplification_norm: f64,
    pub clarity_need_norm: f64,
    pub temperature: f64,
    pub arc: SituationArc,
    pub clarity: Clarity,
    /// Updated narrative centroid embedding (dampened).
    pub narrative_centroid: Option<Vec<f32>>,
    /// Updated geographic centroid.
    pub centroid_lat: Option<f64>,
    pub centroid_lng: Option<f64>,
}

/// Compute all temperature components and derive arc for a situation.
pub async fn compute_temperature(
    client: &GraphClient,
    situation_id: &Uuid,
) -> Result<TemperatureComponents, neo4rs::Error> {
    let sit_id = situation_id.to_string();
    let g = &client.graph;

    // Fetch situation metadata
    let meta_q = query(
        "MATCH (s:Situation {id: $id})
         RETURN s.first_seen AS first_seen, s.arc AS arc, s.last_updated AS last_updated",
    )
    .param("id", sit_id.clone());

    let mut stream = g.execute(meta_q).await?;
    let (first_seen, previous_arc, last_updated) = if let Some(row) = stream.next().await? {
        let first_seen_str: String = row.get("first_seen").unwrap_or_default();
        let first_seen = DateTime::parse_from_rfc3339(&first_seen_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let arc_str: String = row.get("arc").unwrap_or_default();
        let last_updated_str: String = row.get("last_updated").unwrap_or_default();
        let last_updated = DateTime::parse_from_rfc3339(&last_updated_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        (first_seen, arc_str, last_updated)
    } else {
        return Ok(TemperatureComponents {
            tension_heat_agg: 0.0,
            entity_velocity_norm: 0.0,
            response_gap_norm: 0.0,
            amplification_norm: 0.0,
            clarity_need_norm: 0.0,
            temperature: 0.0,
            arc: SituationArc::Cold,
            clarity: Clarity::Fuzzy,
            narrative_centroid: None,
            centroid_lat: None,
            centroid_lng: None,
        });
    };

    let tension_heat_agg = compute_tension_heat_agg(g, &sit_id).await?;
    let entity_velocity_norm = compute_entity_velocity(g, &sit_id).await?;
    let response_gap_norm = compute_response_gap(g, &sit_id).await?;
    let amplification_norm = compute_amplification(g, &sit_id).await?;
    let clarity_need_norm = compute_clarity_need(g, &sit_id, last_updated).await?;

    let substance = (tension_heat_agg + entity_velocity_norm).min(1.0);
    let amplification_contrib = amplification_norm * substance;

    let temperature = 0.30 * tension_heat_agg
        + 0.25 * entity_velocity_norm
        + 0.15 * response_gap_norm
        + 0.15 * amplification_contrib
        + 0.15 * clarity_need_norm;

    let arc = derive_arc(temperature, first_seen, &previous_arc);
    let clarity = derive_clarity(g, &sit_id).await?;

    let (narrative_centroid, centroid_lat, centroid_lng) =
        compute_dampened_centroid(g, &sit_id).await?;

    Ok(TemperatureComponents {
        tension_heat_agg,
        entity_velocity_norm,
        response_gap_norm,
        amplification_norm,
        clarity_need_norm,
        temperature,
        arc,
        clarity,
        narrative_centroid: Some(narrative_centroid),
        centroid_lat,
        centroid_lng,
    })
}

/// Recompute and persist temperature for a situation.
pub async fn recompute_situation_temperature(
    client: &GraphClient,
    writer: &GraphWriter,
    situation_id: &Uuid,
) -> Result<TemperatureComponents, Box<dyn std::error::Error + Send + Sync>> {
    let components = compute_temperature(client, situation_id).await?;

    writer
        .update_situation_temperature(
            situation_id,
            components.temperature,
            components.tension_heat_agg,
            components.entity_velocity_norm,
            components.amplification_norm,
            components.response_gap_norm,
            components.clarity_need_norm,
            &components.arc,
            &components.clarity,
        )
        .await?;

    if let Some(ref centroid) = components.narrative_centroid {
        // Fetch existing causal embedding to preserve it
        let causal = fetch_causal_embedding(&client.graph, &situation_id.to_string())
            .await?
            .unwrap_or_else(|| centroid.clone());
        writer
            .update_situation_embedding(situation_id, centroid, &causal)
            .await?;
    }

    Ok(components)
}

// --- Component computations (all from graph queries) ---

/// Mean cause_heat of non-debunked Tension-type signals in this situation.
async fn compute_tension_heat_agg(
    g: &neo4rs::Graph,
    situation_id: &str,
) -> Result<f64, neo4rs::Error> {
    let q = query(
        "MATCH (t:Tension)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
         RETURN avg(coalesce(t.cause_heat, 0.0)) AS avg_heat,
                count(t) AS cnt",
    )
    .param("id", situation_id);

    let mut stream = g.execute(q).await?;
    if let Some(row) = stream.next().await? {
        let cnt: i64 = row.get("cnt").unwrap_or(0);
        if cnt == 0 {
            return Ok(0.0);
        }
        let avg: f64 = row.get("avg_heat").unwrap_or(0.0);
        Ok(avg.clamp(0.0, 1.0))
    } else {
        Ok(0.0)
    }
}

/// Dual-window entity velocity: max(7-day fast, 30-day slow-burn).
/// Entity = unique source domain/org that appeared in a signal.
async fn compute_entity_velocity(
    g: &neo4rs::Graph,
    situation_id: &str,
) -> Result<f64, neo4rs::Error> {
    let now = Utc::now();
    let seven_days_ago = (now - Duration::days(7)).to_rfc3339();
    let thirty_days_ago = (now - Duration::days(30)).to_rfc3339();

    // Count unique source domains in the 7-day window that weren't present before
    let fast_q = query(
        "MATCH (sig)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND sig.created_at >= $window_start
         WITH collect(DISTINCT sig.source_domain) AS recent_domains
         OPTIONAL MATCH (old_sig)-[e2:PART_OF]->(s2:Situation {id: $id})
         WHERE coalesce(e2.debunked, false) = false
           AND old_sig.created_at < $window_start
         WITH recent_domains, collect(DISTINCT old_sig.source_domain) AS old_domains
         RETURN size([d IN recent_domains WHERE NOT d IN old_domains]) AS net_new",
    )
    .param("id", situation_id)
    .param("window_start", seven_days_ago);

    let fast_velocity = execute_velocity_query(g, fast_q, 3.0).await?;

    // Count unique source domains in the 30-day window
    let slow_q = query(
        "MATCH (sig)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND sig.created_at >= $window_start
         WITH collect(DISTINCT sig.source_domain) AS recent_domains
         OPTIONAL MATCH (old_sig)-[e2:PART_OF]->(s2:Situation {id: $id})
         WHERE coalesce(e2.debunked, false) = false
           AND old_sig.created_at < $window_start
         WITH recent_domains, collect(DISTINCT old_sig.source_domain) AS old_domains
         RETURN size([d IN recent_domains WHERE NOT d IN old_domains]) AS net_new",
    )
    .param("id", situation_id)
    .param("window_start", thirty_days_ago);

    let slow_velocity = execute_velocity_query(g, slow_q, 5.0).await?;

    Ok(fast_velocity.max(slow_velocity).clamp(0.0, 1.0))
}

async fn execute_velocity_query(
    g: &neo4rs::Graph,
    q: neo4rs::Query,
    denominator: f64,
) -> Result<f64, neo4rs::Error> {
    let mut stream = g.execute(q).await?;
    if let Some(row) = stream.next().await? {
        let net_new: i64 = row.get("net_new").unwrap_or(0);
        Ok((net_new as f64 / denominator).min(1.0).max(0.0))
    } else {
        Ok(0.0)
    }
}

/// Ratio of unmet tensions (no RESPONDS_TO) to total tensions, 90-day window.
async fn compute_response_gap(g: &neo4rs::Graph, situation_id: &str) -> Result<f64, neo4rs::Error> {
    let cutoff = (Utc::now() - Duration::days(90)).to_rfc3339();

    let q = query(
        "MATCH (t:Tension)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND t.created_at >= $cutoff
         WITH t
         OPTIONAL MATCH (resp)-[:RESPONDS_TO]->(t)
         RETURN count(DISTINCT t) AS total_tensions,
                count(DISTINCT CASE WHEN resp IS NULL THEN t END) AS unmet_tensions",
    )
    .param("id", situation_id)
    .param("cutoff", cutoff);

    let mut stream = g.execute(q).await?;
    if let Some(row) = stream.next().await? {
        let total: i64 = row.get("total_tensions").unwrap_or(0);
        let unmet: i64 = row.get("unmet_tensions").unwrap_or(0);
        if total == 0 {
            return Ok(0.0);
        }
        Ok((unmet as f64 / total.max(1) as f64).clamp(0.0, 1.0))
    } else {
        Ok(0.0)
    }
}

/// External geographic references: count of signals from outside the primary region
/// that reference this situation's location. Capped at 5.
async fn compute_amplification(
    g: &neo4rs::Graph,
    situation_id: &str,
) -> Result<f64, neo4rs::Error> {
    // Count signals with external geographic references (signals that mention
    // the situation's location but originate from elsewhere)
    let q = query(
        "MATCH (sig)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND sig.external_reference = true
         RETURN count(DISTINCT sig.source_domain) AS external_refs",
    )
    .param("id", situation_id);

    let mut stream = g.execute(q).await?;
    if let Some(row) = stream.next().await? {
        let refs: i64 = row.get("external_refs").unwrap_or(0);
        Ok((refs as f64 / 5.0).min(1.0).max(0.0))
    } else {
        Ok(0.0)
    }
}

/// Graph-derived clarity need: thesis_support × thesis_diversity.
/// With staleness decay after 30 days of no new signals.
async fn compute_clarity_need(
    g: &neo4rs::Graph,
    situation_id: &str,
    last_updated: DateTime<Utc>,
) -> Result<f64, neo4rs::Error> {
    let q = query(
        "MATCH (t:Tension)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND coalesce(t.cause_heat, 0.0) >= 0.5
         RETURN count(t) AS thesis_support,
                count(DISTINCT t.source_domain) AS thesis_diversity",
    )
    .param("id", situation_id);

    let mut stream = g.execute(q).await?;
    let (support, diversity) = if let Some(row) = stream.next().await? {
        let s: i64 = row.get("thesis_support").unwrap_or(0);
        let d: i64 = row.get("thesis_diversity").unwrap_or(0);
        (s, d)
    } else {
        (0, 0)
    };

    let clarity_score = (support as f64 / 3.0).min(1.0) * (diversity as f64 / 2.0).min(1.0);
    let mut clarity_need = 1.0 - clarity_score;

    // Staleness decay: after 30 days of no new signals, decay to 0 over next 60 days
    let days_since_update = (Utc::now() - last_updated).num_days() as f64;
    if days_since_update > 30.0 {
        let decay = (1.0 - (days_since_update - 30.0) / 60.0).max(0.0);
        clarity_need *= decay;
    }

    Ok(clarity_need.clamp(0.0, 1.0))
}

/// Derive arc from temperature + age, evaluated top-to-bottom (first match wins).
pub fn derive_arc(temperature: f64, first_seen: DateTime<Utc>, previous_arc: &str) -> SituationArc {
    let age_hours = (Utc::now() - first_seen).num_hours();

    // Priority 1: Reactivation (was Cold, now warm enough)
    if previous_arc == "cold" && temperature >= 0.3 {
        return SituationArc::Developing; // + Reactivation dispatch emitted by caller
    }

    // Priority 2: Cold
    if temperature < 0.1 {
        return SituationArc::Cold;
    }

    // Priority 3: Cooling
    if temperature < 0.3 {
        return SituationArc::Cooling;
    }

    // Priority 4: Emerging (young + warm)
    if age_hours < 72 {
        return SituationArc::Emerging;
    }

    // Priority 5: Developing
    if temperature < 0.6 {
        return SituationArc::Developing;
    }

    // Priority 6: Active
    SituationArc::Active
}

/// Derive clarity label from graph evidence.
async fn derive_clarity(g: &neo4rs::Graph, situation_id: &str) -> Result<Clarity, neo4rs::Error> {
    let q = query(
        "MATCH (t:Tension)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND coalesce(t.cause_heat, 0.0) >= 0.5
         RETURN count(t) AS support,
                count(DISTINCT t.source_domain) AS diversity",
    )
    .param("id", situation_id);

    let mut stream = g.execute(q).await?;
    if let Some(row) = stream.next().await? {
        let support: i64 = row.get("support").unwrap_or(0);
        let diversity: i64 = row.get("diversity").unwrap_or(0);
        let score = (support as f64 / 3.0).min(1.0) * (diversity as f64 / 2.0).min(1.0);
        Ok(if score < 0.3 {
            Clarity::Fuzzy
        } else if score < 0.7 {
            Clarity::Sharpening
        } else {
            Clarity::Sharp
        })
    } else {
        Ok(Clarity::Fuzzy)
    }
}

/// Fetch existing causal embedding from a situation node.
async fn fetch_causal_embedding(
    g: &neo4rs::Graph,
    situation_id: &str,
) -> Result<Option<Vec<f32>>, neo4rs::Error> {
    let q = query(
        "MATCH (s:Situation {id: $id})
         RETURN s.causal_embedding AS emb",
    )
    .param("id", situation_id);
    let mut stream = g.execute(q).await?;
    if let Some(row) = stream.next().await? {
        let emb: Vec<f32> = row.get("emb").unwrap_or_default();
        if emb.is_empty() {
            Ok(None)
        } else {
            Ok(Some(emb))
        }
    } else {
        Ok(None)
    }
}

/// Dampened rolling centroid: recency + cause_heat weighted mean embedding.
/// Also returns geographic centroid.
async fn compute_dampened_centroid(
    g: &neo4rs::Graph,
    situation_id: &str,
) -> Result<(Vec<f32>, Option<f64>, Option<f64>), neo4rs::Error> {
    let q = query(
        "MATCH (sig)-[e:PART_OF]->(s:Situation {id: $id})
         WHERE coalesce(e.debunked, false) = false
           AND sig.embedding IS NOT NULL
         RETURN sig.embedding AS embedding,
                coalesce(sig.cause_heat, 0.0) AS cause_heat,
                sig.created_at AS created_at,
                sig.lat AS lat, sig.lng AS lng",
    )
    .param("id", situation_id);

    let mut stream = g.execute(q).await?;
    let mut signals: Vec<(Vec<f32>, f64, DateTime<Utc>, Option<f64>, Option<f64>)> = Vec::new();

    while let Some(row) = stream.next().await? {
        let embedding: Vec<f32> = row.get("embedding").unwrap_or_default();
        if embedding.is_empty() {
            continue;
        }
        let cause_heat: f64 = row.get("cause_heat").unwrap_or(0.0);
        let created_str: String = row.get("created_at").unwrap_or_default();
        let created = DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let lat: Option<f64> = row.get("lat").ok();
        let lng: Option<f64> = row.get("lng").ok();
        signals.push((embedding, cause_heat, created, lat, lng));
    }

    if signals.is_empty() {
        return Ok((vec![0.0; 1024], None, None));
    }

    let now = Utc::now();
    let dim = signals[0].0.len();
    let mut centroid = vec![0.0f64; dim];
    let mut total_weight = 0.0f64;
    let mut lat_sum = 0.0f64;
    let mut lng_sum = 0.0f64;
    let mut geo_weight = 0.0f64;

    for (emb, cause_heat, created, lat, lng) in &signals {
        let days_old = (now - *created).num_hours() as f64 / 24.0;
        let recency_weight = (-0.03 * days_old).exp(); // half-life ~23 days
        let heat_weight = 0.3 + 0.7 * cause_heat; // floor of 0.3
        let weight = recency_weight * heat_weight;

        for (i, v) in emb.iter().enumerate() {
            if i < dim {
                centroid[i] += weight * (*v as f64);
            }
        }
        total_weight += weight;

        if let (Some(la), Some(ln)) = (lat, lng) {
            lat_sum += weight * la;
            lng_sum += weight * ln;
            geo_weight += weight;
        }
    }

    let embedding: Vec<f32> = if total_weight > 0.0 {
        centroid
            .iter()
            .map(|v| (*v / total_weight) as f32)
            .collect()
    } else {
        vec![0.0; dim]
    };

    let centroid_lat = if geo_weight > 0.0 {
        Some(lat_sum / geo_weight)
    } else {
        None
    };
    let centroid_lng = if geo_weight > 0.0 {
        Some(lng_sum / geo_weight)
    } else {
        None
    };

    Ok((embedding, centroid_lat, centroid_lng))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_arc_cold_when_low_temp() {
        let first_seen = Utc::now() - Duration::days(10);
        assert_eq!(
            derive_arc(0.05, first_seen, "developing").to_string(),
            "cold"
        );
    }

    #[test]
    fn test_derive_arc_cooling() {
        let first_seen = Utc::now() - Duration::days(10);
        assert_eq!(
            derive_arc(0.15, first_seen, "developing").to_string(),
            "cooling"
        );
    }

    #[test]
    fn test_derive_arc_emerging_young_warm() {
        // 12 hours old, high temp → still Emerging
        let first_seen = Utc::now() - Duration::hours(12);
        assert_eq!(derive_arc(0.93, first_seen, "").to_string(), "emerging");
    }

    #[test]
    fn test_derive_arc_developing_old_moderate() {
        let first_seen = Utc::now() - Duration::days(5);
        assert_eq!(
            derive_arc(0.45, first_seen, "emerging").to_string(),
            "developing"
        );
    }

    #[test]
    fn test_derive_arc_active_old_hot() {
        let first_seen = Utc::now() - Duration::days(5);
        assert_eq!(
            derive_arc(0.75, first_seen, "developing").to_string(),
            "active"
        );
    }

    #[test]
    fn test_derive_arc_reactivation_from_cold() {
        let first_seen = Utc::now() - Duration::days(60);
        // Was cold, now warm → Developing (reactivation)
        assert_eq!(
            derive_arc(0.35, first_seen, "cold").to_string(),
            "developing"
        );
    }

    #[test]
    fn test_derive_arc_dead_cat_bounce() {
        let first_seen = Utc::now() - Duration::days(60);
        // Was cold, weak bounce at 0.2 → Cooling (not reactivation)
        assert_eq!(derive_arc(0.20, first_seen, "cold").to_string(), "cooling");
    }

    #[test]
    fn test_derive_arc_boundary_at_03() {
        let first_seen = Utc::now() - Duration::days(10);
        // At exactly 0.3, old situation → Developing
        assert_eq!(
            derive_arc(0.3, first_seen, "cooling").to_string(),
            "developing"
        );
    }

    #[test]
    fn test_derive_arc_boundary_at_06() {
        let first_seen = Utc::now() - Duration::days(10);
        // At exactly 0.6 → Active
        assert_eq!(
            derive_arc(0.6, first_seen, "developing").to_string(),
            "active"
        );
    }

    #[test]
    fn test_derive_arc_exactly_72h_is_not_emerging() {
        // At exactly 72h, first_seen >= 72h is true → not Emerging
        let first_seen = Utc::now() - Duration::hours(72);
        assert_eq!(derive_arc(0.5, first_seen, "").to_string(), "developing");
    }

    #[test]
    fn test_derive_arc_71h_is_emerging() {
        let first_seen = Utc::now() - Duration::hours(71);
        assert_eq!(derive_arc(0.5, first_seen, "").to_string(), "emerging");
    }

    #[test]
    fn test_temperature_formula_no_amplification_without_substance() {
        // Amplification is multiplicative with substance
        let tension_heat: f64 = 0.0;
        let entity_velocity: f64 = 0.0;
        let response_gap: f64 = 0.0;
        let amplification: f64 = 1.0; // Max amplification
        let clarity_need: f64 = 0.5;

        let substance = (tension_heat + entity_velocity).min(1.0);
        let amplification_contrib = amplification * substance;

        let temperature = 0.30 * tension_heat
            + 0.25 * entity_velocity
            + 0.15 * response_gap
            + 0.15 * amplification_contrib
            + 0.15 * clarity_need;

        // Only clarity_need contributes: 0.15 * 0.5 = 0.075
        assert!((temperature - 0.075).abs() < 0.001);
        // Amplification = 0 because substance = 0
        assert_eq!(amplification_contrib, 0.0);
    }

    #[test]
    fn test_temperature_formula_max_components() {
        let tension_heat: f64 = 1.0;
        let entity_velocity: f64 = 1.0;
        let response_gap: f64 = 1.0;
        let amplification: f64 = 1.0;
        let clarity_need: f64 = 1.0;

        let substance = (tension_heat + entity_velocity).min(1.0); // capped at 1.0
        let amplification_contrib = amplification * substance; // 1.0

        let temperature = 0.30 * tension_heat
            + 0.25 * entity_velocity
            + 0.15 * response_gap
            + 0.15 * amplification_contrib
            + 0.15 * clarity_need;

        // 0.30 + 0.25 + 0.15 + 0.15 + 0.15 = 1.0
        assert!((temperature - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_temperature_formula_weights_sum_to_one() {
        // When all components = 1.0 and amplification substance = 1.0
        let weights: f64 = 0.30 + 0.25 + 0.15 + 0.15 + 0.15;
        assert!((weights - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_amplification_multiplicative_with_substance() {
        // High amplification but low substance → low contribution
        let tension_heat: f64 = 0.1;
        let entity_velocity: f64 = 0.1;
        let amplification: f64 = 1.0;

        let substance = (tension_heat + entity_velocity).min(1.0); // 0.2
        let contrib = amplification * substance; // 0.2

        assert!((contrib - 0.2).abs() < 0.001);
    }
}
