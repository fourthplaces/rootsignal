use std::collections::HashMap;

use neo4rs::query;
use tracing::info;
use uuid::Uuid;

use rootsignal_common::{ScoutTask, ScoutTaskSource, ScoutTaskStatus};

use crate::{GraphClient, GraphWriter};

/// Detect geographic clusters of recent signals and create ScoutTasks for hot areas.
///
/// Algorithm:
/// 1. Query all signals from last 7 days with lat/lng
/// 2. Bucket into geohash-5 cells (~5km precision)
/// 3. For each cell with ≥ 3 signals, check if a pending/running task already covers it
/// 4. If not, create a ScoutTask with source: Beacon
pub async fn detect_beacons(
    client: &GraphClient,
    writer: &GraphWriter,
) -> Result<Vec<ScoutTask>, neo4rs::Error> {
    // Query recent signals with coordinates
    let q = query(
        "MATCH (s)
         WHERE (s:Gathering OR s:Aid OR s:Need OR s:Notice OR s:Tension)
           AND s.lat IS NOT NULL AND s.lng IS NOT NULL
           AND s.extracted_at > datetime() - duration('P7D')
         RETURN s.lat AS lat, s.lng AS lng, s.title AS title",
    );

    let mut signals: Vec<(f64, f64, String)> = Vec::new();
    let mut stream = client.graph.execute(q).await?;
    while let Some(row) = stream.next().await? {
        let lat: f64 = row.get("lat").unwrap_or(0.0);
        let lng: f64 = row.get("lng").unwrap_or(0.0);
        let title: String = row.get("title").unwrap_or_default();
        if lat.abs() > 0.01 || lng.abs() > 0.01 {
            signals.push((lat, lng, title));
        }
    }

    if signals.is_empty() {
        return Ok(Vec::new());
    }

    // Bucket into geohash-5 cells
    // Note: geohash::encode takes (lng, lat) order via Coord { x: lng, y: lat }
    let mut cells: HashMap<String, Vec<&(f64, f64, String)>> = HashMap::new();
    for sig in &signals {
        // geohash::encode takes Coord { x: lng, y: lat }
        if let Ok(hash) = geohash::encode(
            geohash::Coord { x: sig.1, y: sig.0 },
            5,
        ) {
            cells.entry(hash).or_default().push(sig);
        }
    }

    // Get existing pending/running tasks to avoid duplicates
    let existing_tasks = writer.list_scout_tasks(Some("pending"), 100).await?;
    let mut running_tasks = writer.list_scout_tasks(Some("running"), 100).await?;
    running_tasks.extend(existing_tasks);

    let existing_hashes: std::collections::HashSet<String> = running_tasks
        .iter()
        .filter_map(|t| {
            geohash::encode(
                geohash::Coord { x: t.center_lng, y: t.center_lat },
                5,
            )
            .ok()
        })
        .collect();

    let mut new_tasks = Vec::new();

    for (hash, cell_signals) in &cells {
        if cell_signals.len() < 3 {
            continue;
        }

        // Skip if we already have a task in this cell
        if existing_hashes.contains(hash) {
            continue;
        }

        // Compute centroid
        let n = cell_signals.len() as f64;
        let avg_lat: f64 = cell_signals.iter().map(|s| s.0).sum::<f64>() / n;
        let avg_lng: f64 = cell_signals.iter().map(|s| s.1).sum::<f64>() / n;

        // Priority based on signal density
        let priority = (cell_signals.len() as f64 / 10.0).min(1.0);

        let context = format!(
            "Beacon: {} signals near ({:.3}, {:.3})",
            cell_signals.len(),
            avg_lat,
            avg_lng
        );

        let task = ScoutTask {
            id: Uuid::new_v4(),
            center_lat: avg_lat,
            center_lng: avg_lng,
            radius_km: 10.0, // ~5km geohash cell, use 10km radius for coverage
            context,
            geo_terms: Vec::new(),
            priority,
            source: ScoutTaskSource::Beacon,
            status: ScoutTaskStatus::Pending,
            created_at: chrono::Utc::now(),
            completed_at: None,
        };

        writer.upsert_scout_task(&task).await?;
        new_tasks.push(task);
    }

    info!(
        total_signals = signals.len(),
        cells = cells.len(),
        new_tasks = new_tasks.len(),
        "Beacon detection complete"
    );
    Ok(new_tasks)
}
