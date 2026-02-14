use anyhow::Result;
use chrono::{DateTime, Datelike, Timelike, Utc};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use taproot_core::ServerDeps;
use uuid::Uuid;

use crate::clustering::{Cluster, ClusterItem};

/// Stats returned by the clustering job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStats {
    pub items_processed: u32,
    pub clusters_created: u32,
    pub clusters_merged: u32,
    pub items_assigned: u32,
}

/// A listing row with the fields needed for clustering.
#[derive(Debug, Clone, sqlx::FromRow)]
struct ClusterCandidate {
    id: Uuid,
    title: String,
    entity_id: Option<Uuid>,
    location_text: Option<String>,
    latitude: Option<f32>,
    longitude: Option<f32>,
    timing_start: Option<DateTime<Utc>>,
    is_recurring: bool,
}

/// A neighbor found via ANN search.
#[derive(Debug, sqlx::FromRow)]
struct AnnCandidate {
    id: Uuid,
    title: String,
    entity_id: Option<Uuid>,
    location_text: Option<String>,
    latitude: Option<f32>,
    longitude: Option<f32>,
    timing_start: Option<DateTime<Utc>>,
    is_recurring: bool,
    similarity: f64,
}

/// Run one batch of listing clustering.
pub async fn cluster_listings(deps: &ServerDeps) -> Result<ClusterStats> {
    let pool = deps.pool();
    let config = &deps.config;

    let similarity_threshold = config.cluster_similarity_threshold;
    let match_score_threshold = config.cluster_match_score_threshold;
    let _merge_threshold = config.cluster_merge_coherence_threshold;
    let geo_radius = config.cluster_geo_radius_meters;
    let time_window = config.cluster_time_window_hours;
    let batch_size = config.cluster_batch_size;
    let ef_search = config.hnsw_ef_search;

    // Set ef_search for better recall during dedup
    sqlx::query(&format!("SET LOCAL hnsw.ef_search = {}", ef_search))
        .execute(pool)
        .await?;

    // Fetch unclustered listings with embeddings
    let unclustered_ids = ClusterItem::unclustered("listing", batch_size, pool).await?;

    if unclustered_ids.is_empty() {
        return Ok(ClusterStats {
            items_processed: 0,
            clusters_created: 0,
            clusters_merged: 0,
            items_assigned: 0,
        });
    }

    let mut stats = ClusterStats {
        items_processed: unclustered_ids.len() as u32,
        clusters_created: 0,
        clusters_merged: 0,
        items_assigned: 0,
    };

    // Track which items should be grouped together for union-find
    // Key: index in unclustered_ids, Value: cluster_id they should join
    let mut assignments: HashMap<Uuid, Uuid> = HashMap::new();

    // Process each unclustered listing
    for &item_id in &unclustered_ids {
        // Load the item's data
        let item = sqlx::query_as::<_, ClusterCandidate>(
            r#"
            SELECT l.id, l.title, l.entity_id, l.location_text, l.latitude, l.longitude,
                   l.timing_start,
                   COALESCE((SELECT t.value = 'true' FROM taggables tg
                             JOIN tags t ON t.id = tg.tag_id
                             WHERE tg.taggable_type = 'listing' AND tg.taggable_id = l.id
                             AND t.kind = 'is_recurring'), false) as is_recurring
            FROM listings l
            WHERE l.id = $1
            "#,
        )
        .bind(item_id)
        .fetch_one(pool)
        .await?;

        // Get the item's embedding
        let embedding = sqlx::query_as::<_, (Vector,)>(
            "SELECT embedding FROM listings WHERE id = $1 AND embedding IS NOT NULL",
        )
        .bind(item_id)
        .fetch_one(pool)
        .await?;

        // ANN search using MATERIALIZED CTE for HNSW index utilization
        let cosine_distance_threshold = 1.0 - similarity_threshold;
        let neighbors = sqlx::query_as::<_, AnnCandidate>(
            r#"
            WITH candidates AS MATERIALIZED (
                SELECT l.id, l.title, l.entity_id, l.location_text, l.latitude, l.longitude,
                       l.timing_start,
                       COALESCE((SELECT t.value = 'true' FROM taggables tg
                                 JOIN tags t ON t.id = tg.tag_id
                                 WHERE tg.taggable_type = 'listing' AND tg.taggable_id = l.id
                                 AND t.kind = 'is_recurring'), false) as is_recurring,
                       l.embedding <=> $1 AS distance
                FROM listings l
                WHERE l.embedding IS NOT NULL
                  AND l.id != $2
                ORDER BY l.embedding <=> $1
                LIMIT 50
            )
            SELECT id, title, entity_id, location_text, latitude, longitude,
                   timing_start, is_recurring,
                   (1.0 - distance)::float8 AS similarity
            FROM candidates
            WHERE distance < $3
            "#,
        )
        .bind(&embedding.0)
        .bind(item_id)
        .bind(cosine_distance_threshold)
        .fetch_all(pool)
        .await?;

        // Score each neighbor with composite match scoring
        let mut best_match: Option<(Uuid, f64, Option<Uuid>)> = None; // (neighbor_id, score, cluster_id)

        for neighbor in &neighbors {
            let score = composite_match_score(&item, neighbor, geo_radius, time_window);

            if score < match_score_threshold {
                continue;
            }

            // Look up which cluster this neighbor belongs to
            let neighbor_cluster =
                ClusterItem::find_cluster_for("listing", neighbor.id, pool).await?;

            let cluster_id = neighbor_cluster.map(|ci| ci.cluster_id);

            if let Some((_, best_score, _)) = &best_match {
                if score > *best_score {
                    best_match = Some((neighbor.id, score, cluster_id));
                }
            } else {
                best_match = Some((neighbor.id, score, cluster_id));
            }
        }

        match best_match {
            Some((_neighbor_id, score, Some(cluster_id))) => {
                // Assign to existing cluster
                ClusterItem::create(
                    cluster_id,
                    item_id,
                    "listing",
                    Some(score as f32),
                    pool,
                )
                .await?;
                assignments.insert(item_id, cluster_id);
                stats.items_assigned += 1;

                // Recompute representative
                recompute_representative(cluster_id, pool).await?;
            }
            Some((neighbor_id, score, None)) => {
                // Neighbor exists but has no cluster — check if neighbor was already
                // assigned in this batch
                if let Some(&existing_cluster_id) = assignments.get(&neighbor_id) {
                    ClusterItem::create(
                        existing_cluster_id,
                        item_id,
                        "listing",
                        Some(score as f32),
                        pool,
                    )
                    .await?;
                    assignments.insert(item_id, existing_cluster_id);
                    stats.items_assigned += 1;
                    recompute_representative(existing_cluster_id, pool).await?;
                } else {
                    // Create new cluster with the neighbor as initial representative
                    let cluster =
                        Cluster::create("listing", neighbor_id, pool).await?;
                    ClusterItem::create(
                        cluster.id,
                        item_id,
                        "listing",
                        Some(score as f32),
                        pool,
                    )
                    .await?;
                    assignments.insert(neighbor_id, cluster.id);
                    assignments.insert(item_id, cluster.id);
                    stats.clusters_created += 1;
                    stats.items_assigned += 1;
                    recompute_representative(cluster.id, pool).await?;
                }
            }
            None => {
                // No matches — create singleton cluster
                let cluster = Cluster::create("listing", item_id, pool).await?;
                assignments.insert(item_id, cluster.id);
                stats.clusters_created += 1;
            }
        }
    }

    // Check for cluster merges: if any item in this batch matched items in multiple clusters,
    // the assignments map may have items that should be in the same cluster.
    // For now, merges happen naturally via the assignment loop above.
    // A full connected-components merge pass would be needed for more complex transitive merges.

    tracing::info!(
        items_processed = stats.items_processed,
        clusters_created = stats.clusters_created,
        items_assigned = stats.items_assigned,
        clusters_merged = stats.clusters_merged,
        "Clustering batch complete"
    );

    Ok(stats)
}

/// Compute a composite match score between an unclustered item and a neighbor.
fn composite_match_score(
    item: &ClusterCandidate,
    neighbor: &AnnCandidate,
    geo_radius_meters: f64,
    time_window_hours: i64,
) -> f64 {
    let embedding_sim = neighbor.similarity;

    let geo_score = geo_proximity_score(
        item.latitude,
        item.longitude,
        item.location_text.as_deref(),
        neighbor.latitude,
        neighbor.longitude,
        neighbor.location_text.as_deref(),
        geo_radius_meters,
    );

    let time_score = temporal_proximity_score(
        item.timing_start,
        item.is_recurring,
        neighbor.timing_start,
        neighbor.is_recurring,
        time_window_hours,
    );

    let name_sim = strsim::jaro_winkler(&item.title, &neighbor.title);

    let org_match = match (item.entity_id, neighbor.entity_id) {
        (Some(a), Some(b)) if a == b => 1.0,
        _ => 0.0,
    };

    embedding_sim * 0.40 + geo_score * 0.25 + time_score * 0.15 + name_sim * 0.10 + org_match * 0.10
}

/// Geographic proximity score (0-1).
fn geo_proximity_score(
    lat1: Option<f32>,
    lon1: Option<f32>,
    loc_text1: Option<&str>,
    lat2: Option<f32>,
    lon2: Option<f32>,
    loc_text2: Option<&str>,
    radius_meters: f64,
) -> f64 {
    match (lat1, lon1, lat2, lon2) {
        (Some(la1), Some(lo1), Some(la2), Some(lo2)) => {
            let distance = haversine_meters(la1 as f64, lo1 as f64, la2 as f64, lo2 as f64);
            if distance <= 100.0 {
                1.0
            } else if distance >= radius_meters {
                0.0
            } else {
                1.0 - (distance - 100.0) / (radius_meters - 100.0)
            }
        }
        _ => {
            // Fall back to location text similarity
            match (loc_text1, loc_text2) {
                (Some(t1), Some(t2)) => strsim::jaro_winkler(t1, t2),
                (None, None) => 0.5, // Both lack location — neutral
                _ => 0.3,            // One has location, other doesn't
            }
        }
    }
}

/// Temporal proximity score (0-1).
fn temporal_proximity_score(
    time1: Option<DateTime<Utc>>,
    recurring1: bool,
    time2: Option<DateTime<Utc>>,
    recurring2: bool,
    window_hours: i64,
) -> f64 {
    match (time1, time2) {
        (Some(t1), Some(t2)) => {
            if recurring1 && recurring2 {
                // For recurring events: compare day-of-week and time-of-day
                let same_weekday = t1.weekday() == t2.weekday();
                let hour_diff = (t1.hour() as i32 - t2.hour() as i32).unsigned_abs();
                let similar_time = hour_diff <= 1;
                if same_weekday && similar_time {
                    1.0
                } else if same_weekday {
                    0.5
                } else {
                    0.2
                }
            } else {
                let diff_hours = (t1 - t2).num_hours().unsigned_abs() as f64;
                if diff_hours <= 0.0 {
                    1.0
                } else if diff_hours >= window_hours as f64 {
                    0.0
                } else {
                    1.0 - diff_hours / window_hours as f64
                }
            }
        }
        (None, None) => 1.0, // Both ongoing services
        _ => 0.3,            // One has timing, other doesn't
    }
}

/// Haversine distance in meters between two lat/lon points.
fn haversine_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0; // Earth radius in meters
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    r * c
}

/// Recompute the representative for a cluster based on field completeness and provenance.
async fn recompute_representative(cluster_id: Uuid, pool: &PgPool) -> Result<()> {
    // Select the best representative: most non-null fields + most provenance
    let best = sqlx::query_as::<_, (Uuid,)>(
        r#"
        SELECT ci.item_id
        FROM cluster_items ci
        JOIN listings l ON l.id = ci.item_id
        WHERE ci.cluster_id = $1 AND ci.item_type = 'listing' AND l.status = 'active'
        ORDER BY
            -- Field completeness: count non-null optional fields
            (CASE WHEN l.description IS NOT NULL THEN 1 ELSE 0 END
             + CASE WHEN l.location_text IS NOT NULL THEN 1 ELSE 0 END
             + CASE WHEN l.entity_id IS NOT NULL THEN 1 ELSE 0 END
             + CASE WHEN l.service_id IS NOT NULL THEN 1 ELSE 0 END
             + CASE WHEN l.source_url IS NOT NULL THEN 1 ELSE 0 END
             + CASE WHEN l.timing_start IS NOT NULL THEN 1 ELSE 0 END
             + CASE WHEN l.latitude IS NOT NULL THEN 1 ELSE 0 END
            ) DESC,
            -- Provenance count (corroboration)
            (SELECT COUNT(*) FROM listing_extractions le WHERE le.listing_id = l.id) DESC,
            -- Recency
            l.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(cluster_id)
    .fetch_optional(pool)
    .await?;

    if let Some((best_id,)) = best {
        Cluster::update_representative(cluster_id, best_id, pool).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_same_point() {
        let d = haversine_meters(44.9778, -93.2650, 44.9778, -93.2650);
        assert!(d < 0.01);
    }

    #[test]
    fn test_haversine_known_distance() {
        // Minneapolis to St Paul (~15km)
        let d = haversine_meters(44.9778, -93.2650, 44.9537, -93.0900);
        assert!(d > 10_000.0 && d < 20_000.0);
    }

    #[test]
    fn test_geo_score_close() {
        let score = geo_proximity_score(
            Some(44.9778), Some(-93.2650), None,
            Some(44.9779), Some(-93.2651), None,
            500.0,
        );
        assert!(score > 0.9);
    }

    #[test]
    fn test_geo_score_far() {
        // Minneapolis to St Paul
        let score = geo_proximity_score(
            Some(44.9778), Some(-93.2650), None,
            Some(44.9537), Some(-93.0900), None,
            500.0,
        );
        assert!(score < 0.01);
    }

    #[test]
    fn test_geo_score_text_fallback() {
        let score = geo_proximity_score(
            None, None, Some("Minneapolis, MN"),
            None, None, Some("Minneapolis, MN"),
            500.0,
        );
        assert!(score > 0.9);
    }

    #[test]
    fn test_temporal_same_day() {
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::hours(2);
        let score = temporal_proximity_score(Some(t1), false, Some(t2), false, 24);
        assert!(score > 0.9);
    }

    #[test]
    fn test_temporal_different_days() {
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::hours(48);
        let score = temporal_proximity_score(Some(t1), false, Some(t2), false, 24);
        assert!(score < 0.01);
    }

    #[test]
    fn test_temporal_both_ongoing() {
        let score = temporal_proximity_score(None, false, None, false, 24);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }
}
