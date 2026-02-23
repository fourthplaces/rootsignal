use std::collections::{HashMap, HashSet};

use neo4rs::query;
use tracing::info;
use uuid::Uuid;

use rootsignal_common::{ScoutTask, ScoutTaskSource, ScoutTaskStatus};

use crate::{GraphClient, GraphWriter};

/// A candidate beacon from the news scanner — an article with an extracted location.
pub struct BeaconCandidate {
    pub lat: f64,
    pub lng: f64,
    pub title: String,
    pub location_name: Option<String>,
    pub source_url: String,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Get geohash-5 cells of existing pending/running tasks.
async fn existing_task_hashes(writer: &GraphWriter) -> Result<HashSet<String>, neo4rs::Error> {
    let existing_tasks = writer.list_scout_tasks(Some("pending"), 100).await?;
    let mut running_tasks = writer.list_scout_tasks(Some("running"), 100).await?;
    running_tasks.extend(existing_tasks);

    Ok(running_tasks
        .iter()
        .filter_map(|t| {
            geohash::encode(
                geohash::Coord { x: t.center_lng, y: t.center_lat },
                5,
            )
            .ok()
        })
        .collect())
}

/// Pick the most common location_name from a set of (lat, lng, title, location_name) tuples.
fn most_common_location_name<'a>(
    items: impl Iterator<Item = &'a Option<String>>,
) -> Option<String> {
    let mut name_counts: HashMap<&str, usize> = HashMap::new();
    for name_opt in items {
        if let Some(ref name) = name_opt {
            let name = name.trim();
            if !name.is_empty() {
                *name_counts.entry(name).or_default() += 1;
            }
        }
    }
    name_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(name, _)| name.to_string())
}

// ---------------------------------------------------------------------------
// Beacon detection (from existing signals)
// ---------------------------------------------------------------------------

/// Detect geographic clusters of recent signals and create ScoutTasks for hot areas.
///
/// Algorithm:
/// 1. Query all live signals from last 7 days with lat/lng
/// 2. Bucket into geohash-5 cells (~5km precision)
/// 3. For each cell with >= 3 signals, check if a pending/running task already covers it
/// 4. If not, create a ScoutTask with source: Beacon
pub async fn detect_beacons(
    client: &GraphClient,
    writer: &GraphWriter,
) -> Result<Vec<ScoutTask>, neo4rs::Error> {
    let q = query(
        "MATCH (s)
         WHERE (s:Gathering OR s:Aid OR s:Need OR s:Notice OR s:Tension)
           AND s.lat IS NOT NULL AND s.lng IS NOT NULL
           AND s.extracted_at > datetime() - duration('P7D')
           AND s.review_status = 'live'
         RETURN s.lat AS lat, s.lng AS lng, s.title AS title, s.location_name AS location_name",
    );

    let mut signals: Vec<(f64, f64, String, Option<String>)> = Vec::new();
    let mut stream = client.graph.execute(q).await?;
    while let Some(row) = stream.next().await? {
        let lat: f64 = row.get("lat").unwrap_or(0.0);
        let lng: f64 = row.get("lng").unwrap_or(0.0);
        let title: String = row.get("title").unwrap_or_default();
        let location_name: Option<String> = row.get("location_name").ok();
        if lat.abs() > 0.01 || lng.abs() > 0.01 {
            signals.push((lat, lng, title, location_name));
        }
    }

    if signals.is_empty() {
        return Ok(Vec::new());
    }

    // Bucket into geohash-5 cells
    let mut cells: HashMap<String, Vec<&(f64, f64, String, Option<String>)>> = HashMap::new();
    for sig in &signals {
        if let Ok(hash) = geohash::encode(
            geohash::Coord { x: sig.1, y: sig.0 },
            5,
        ) {
            cells.entry(hash).or_default().push(sig);
        }
    }

    let existing_hashes = existing_task_hashes(writer).await?;
    let mut new_tasks = Vec::new();

    for (hash, cell_signals) in &cells {
        if cell_signals.len() < 3 {
            continue;
        }
        if existing_hashes.contains(hash) {
            continue;
        }

        let n = cell_signals.len() as f64;
        let avg_lat: f64 = cell_signals.iter().map(|s| s.0).sum::<f64>() / n;
        let avg_lng: f64 = cell_signals.iter().map(|s| s.1).sum::<f64>() / n;
        let priority = (cell_signals.len() as f64 / 10.0).min(1.0);

        let area_name = most_common_location_name(cell_signals.iter().map(|s| &s.3));
        let context = match &area_name {
            Some(name) => format!("{} — {} signals detected", name, cell_signals.len()),
            None => format!(
                "({:.3}, {:.3}) — {} signals detected",
                avg_lat, avg_lng, cell_signals.len()
            ),
        };

        let task = ScoutTask {
            id: Uuid::new_v4(),
            center_lat: avg_lat,
            center_lng: avg_lng,
            radius_km: 10.0,
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

// ---------------------------------------------------------------------------
// News scanner beacon creation (from global RSS articles)
// ---------------------------------------------------------------------------

/// Create ScoutTask beacons from news article candidates.
///
/// Buckets candidates into geohash-5 cells, deduplicates against existing tasks,
/// and creates tasks for cells with >= 2 candidates (a single article isn't enough
/// signal, but two independent articles about the same area means something is happening).
pub async fn create_beacons_from_news(
    writer: &GraphWriter,
    candidates: Vec<BeaconCandidate>,
) -> Result<u32, neo4rs::Error> {
    if candidates.is_empty() {
        return Ok(0);
    }

    // Bucket into geohash-5 cells
    let mut cells: HashMap<String, Vec<&BeaconCandidate>> = HashMap::new();
    for candidate in &candidates {
        if let Ok(hash) = geohash::encode(
            geohash::Coord { x: candidate.lng, y: candidate.lat },
            5,
        ) {
            cells.entry(hash).or_default().push(candidate);
        }
    }

    let existing_hashes = existing_task_hashes(writer).await?;
    let mut created = 0u32;

    for (hash, cell_candidates) in &cells {
        // Threshold: 2+ candidates per cell
        if cell_candidates.len() < 2 {
            continue;
        }
        if existing_hashes.contains(hash) {
            continue;
        }

        let n = cell_candidates.len() as f64;
        let avg_lat: f64 = cell_candidates.iter().map(|c| c.lat).sum::<f64>() / n;
        let avg_lng: f64 = cell_candidates.iter().map(|c| c.lng).sum::<f64>() / n;
        let priority = (cell_candidates.len() as f64 / 5.0).min(1.0);

        let area_name = most_common_location_name(
            cell_candidates.iter().map(|c| &c.location_name),
        );

        // Collect geo_terms from location names in this cell
        let geo_terms: Vec<String> = cell_candidates
            .iter()
            .filter_map(|c| c.location_name.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let context = match &area_name {
            Some(name) => format!("{} — {} news articles", name, cell_candidates.len()),
            None => format!(
                "({:.3}, {:.3}) — {} news articles",
                avg_lat, avg_lng, cell_candidates.len()
            ),
        };

        let task = ScoutTask {
            id: Uuid::new_v4(),
            center_lat: avg_lat,
            center_lng: avg_lng,
            radius_km: 10.0,
            context,
            geo_terms,
            priority,
            source: ScoutTaskSource::DriverB,
            status: ScoutTaskStatus::Pending,
            created_at: chrono::Utc::now(),
            completed_at: None,
        };

        writer.upsert_scout_task(&task).await?;
        created += 1;
    }

    info!(
        candidates = candidates.len(),
        cells = cells.len(),
        beacons_created = created,
        "News beacon creation complete"
    );
    Ok(created)
}
