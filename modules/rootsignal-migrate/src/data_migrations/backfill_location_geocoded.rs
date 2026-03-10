//! Backfill LocationGeocoded events for historical WorldEvents with locations.
//!
//! Idempotent: checks for existing LocationGeocoded events before geocoding.
//! Safe to run multiple times — subsequent runs are no-ops.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;
use tracing::info;

use rootsignal_graph::geocoder::{GeocodingLookup, GeocodingResult, MapboxGeocoder};

use crate::{BoxFuture, MigrateContext};

/// Row from the events table containing a signal with locations.
struct SignalWithLocations {
    seq: i64,
    signal_id: String,
    locations: Vec<LocationEntry>,
}

struct LocationEntry {
    name: String,
    bias_lat: Option<f64>,
    bias_lng: Option<f64>,
}

/// Scan for WorldEvents with locations that lack corresponding LocationGeocoded events.
async fn find_ungeocode_locations(pg: &PgPool) -> Result<Vec<SignalWithLocations>> {

    // All signal WorldEvent types
    let signal_types = [
        "world:gathering_announced",
        "world:resource_offered",
        "world:help_requested",
        "world:announcement_shared",
        "world:concern_raised",
        "world:condition_observed",
    ];

    // Collect all signal_ids that already have LocationGeocoded events
    let geocoded_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT payload->>'signal_id' AS signal_id
         FROM events
         WHERE event_type = 'system:location_geocoded'"
    )
    .fetch_all(pg)
    .await?;

    let already_geocoded: HashSet<String> = geocoded_rows
        .into_iter()
        .map(|(id,)| id)
        .collect();

    let mut signals = Vec::new();

    for event_type in signal_types {
        let rows: Vec<(i64, serde_json::Value)> = sqlx::query_as(
            "SELECT seq, payload FROM events
             WHERE event_type = $1
               AND jsonb_array_length(COALESCE(payload->'locations', '[]'::jsonb)) > 0
             ORDER BY seq"
        )
        .bind(event_type)
        .fetch_all(pg)
        .await?;

        for (seq, payload) in rows {
            let signal_id = payload.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            if already_geocoded.contains(&signal_id) {
                continue;
            }

            let locations = payload.get("locations")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter().filter_map(|loc| {
                        let name = loc.get("name")?.as_str()?.trim().to_string();
                        if name.is_empty() { return None; }

                        let bias_lat = loc.get("point")
                            .and_then(|p| p.get("lat"))
                            .and_then(|v| v.as_f64());
                        let bias_lng = loc.get("point")
                            .and_then(|p| p.get("lng"))
                            .and_then(|v| v.as_f64());

                        Some(LocationEntry { name, bias_lat, bias_lng })
                    }).collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if !locations.is_empty() {
                signals.push(SignalWithLocations { seq, signal_id, locations });
            }
        }
    }

    Ok(signals)
}

pub fn plan(ctx: &MigrateContext) -> BoxFuture<Result<String>> {
    let pg = ctx.pg().clone();
    Box::pin(async move {
        let signals = find_ungeocode_locations(&pg).await?;

        // Deduplicate location names
        let mut unique_names = HashSet::new();
        for s in &signals {
            for loc in &s.locations {
                unique_names.insert(loc.name.trim().to_lowercase());
            }
        }

        Ok(format!(
            "{} signals with {} unique location names to geocode",
            signals.len(),
            unique_names.len(),
        ))
    })
}

pub fn run(ctx: &MigrateContext) -> BoxFuture<Result<()>> {
    let geocoder = ctx.get::<Arc<MapboxGeocoder>>().cloned();
    let pg = ctx.pg().clone();
    Box::pin(async move {
        let geocoder = geocoder?;

        let signals = find_ungeocode_locations(&pg).await?;
        if signals.is_empty() {
            info!("No locations to geocode");
            return Ok(());
        }

        // Deduplicate: geocode each unique name once
        let mut cache: HashMap<String, Option<GeocodingResult>> = HashMap::new();
        let mut unique_names = Vec::new();
        for s in &signals {
            for loc in &s.locations {
                let key = loc.name.trim().to_lowercase();
                if !cache.contains_key(&key) {
                    cache.insert(key.clone(), None);
                    unique_names.push((key, loc.bias_lat, loc.bias_lng));
                }
            }
        }

        info!(count = unique_names.len(), "Geocoding unique location names");
        for (name, bias_lat, bias_lng) in &unique_names {
            match geocoder.geocode(name, *bias_lat, *bias_lng).await {
                Ok(result) => {
                    cache.insert(name.clone(), result);
                }
                Err(e) => {
                    tracing::warn!(name = name.as_str(), error = %e, "Geocoding failed, skipping");
                }
            }
        }

        // Emit LocationGeocoded events
        let mut emitted = 0u64;
        for signal in &signals {
            for loc in &signal.locations {
                let key = loc.name.trim().to_lowercase();
                let result = match cache.get(&key) {
                    Some(Some(r)) => r,
                    _ => continue,
                };

                let payload = serde_json::json!({
                    "type": "location_geocoded",
                    "signal_id": signal.signal_id,
                    "location_name": loc.name,
                    "lat": result.lat,
                    "lng": result.lng,
                    "address": result.address,
                    "precision": result.precision,
                    "timezone": result.timezone,
                });

                sqlx::query(
                    "INSERT INTO events (event_type, caused_by_seq, payload, actor, schema_v)
                     VALUES ('system:location_geocoded', $1, $2, 'migrate:038', 1)"
                )
                .bind(signal.seq)
                .bind(&payload)
                .execute(&pg)
                .await?;

                emitted += 1;
            }
        }

        info!(emitted, "LocationGeocoded events appended");

        // Notify projection stream
        sqlx::query("SELECT pg_notify('events', '')")
            .execute(&pg)
            .await?;

        Ok(())
    })
}
