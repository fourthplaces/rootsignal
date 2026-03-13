//! Backfill RegionDiscovered events from existing LocationGeocoded events.
//!
//! Scans LocationGeocoded events for city/state/country_name context.
//! For events missing context (pre-Mapbox-v6), re-geocodes the address
//! to obtain the hierarchy. Groups by unique hierarchy, geocodes center
//! coordinates, and emits RegionDiscovered events for each scale.
//!
//! Region names are hierarchical for disambiguation:
//!   country: "United States"
//!   state:   "Minnesota, United States"
//!   city:    "Minneapolis, Minnesota"
//!
//! Idempotent: checks for existing RegionDiscovered events by name.
//! Safe to run multiple times — subsequent runs are no-ops.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use rootsignal_graph::geocoder::{GeocodingLookup, MapboxGeocoder};

use crate::{BoxFuture, MigrateContext};

struct LocationContext {
    city: Option<String>,
    state: Option<String>,
    country_name: Option<String>,
}

/// Extract unique geographic hierarchies from LocationGeocoded events.
/// Re-geocodes locations missing context fields to obtain city/state/country_name.
async fn find_undiscovered_regions(
    pg: &PgPool,
    geocoder: Option<&MapboxGeocoder>,
) -> Result<(Vec<LocationContext>, HashSet<String>)> {
    let discovered_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT payload->>'name' AS name
         FROM events
         WHERE event_type = 'system:region_discovered'"
    )
    .fetch_all(pg)
    .await?;

    let already_discovered: HashSet<String> = discovered_rows
        .into_iter()
        .map(|(name,)| name)
        .collect();

    let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
        "SELECT payload FROM events
         WHERE event_type = 'system:location_geocoded'
         ORDER BY seq"
    )
    .fetch_all(pg)
    .await?;

    let mut regeocode_cache: HashMap<String, Option<LocationContext>> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut contexts = Vec::new();

    for (payload,) in &rows {
        let mut city = payload.get("city").and_then(|v| v.as_str()).map(|s| s.to_string());
        let mut state = payload.get("state").and_then(|v| v.as_str()).map(|s| s.to_string());
        let mut country_name = payload.get("country_name").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Re-geocode if context is missing
        if city.is_none() && state.is_none() && country_name.is_none() {
            if let Some(geocoder) = geocoder {
                let search = payload.get("address")
                    .and_then(|v| v.as_str())
                    .or_else(|| payload.get("location_name").and_then(|v| v.as_str()));

                if let Some(search) = search {
                    let search_key = search.to_string();
                    if !regeocode_cache.contains_key(&search_key) {
                        let result = match geocoder.geocode(search, None, None).await {
                            Ok(Some(r)) => Some(LocationContext {
                                city: r.city,
                                state: r.state,
                                country_name: r.country_name,
                            }),
                            Ok(None) => None,
                            Err(e) => {
                                tracing::warn!(search, error = %e, "Re-geocode failed");
                                None
                            }
                        };
                        regeocode_cache.insert(search_key.clone(), result);
                    }

                    if let Some(Some(cached)) = regeocode_cache.get(&search_key) {
                        city = cached.city.clone();
                        state = cached.state.clone();
                        country_name = cached.country_name.clone();
                    }
                }
            }
        }

        if city.is_none() && state.is_none() && country_name.is_none() {
            continue;
        }

        let key = format!(
            "{}|{}|{}",
            city.as_deref().unwrap_or(""),
            state.as_deref().unwrap_or(""),
            country_name.as_deref().unwrap_or("")
        );

        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);

        // Check if all hierarchical names already exist
        let names = hierarchical_names(&city, &state, &country_name);
        let all_exist = names.iter().all(|n| already_discovered.contains(n));
        if all_exist {
            continue;
        }

        contexts.push(LocationContext { city, state, country_name });
    }

    if !regeocode_cache.is_empty() {
        let hits = regeocode_cache.values().filter(|v| v.is_some()).count();
        info!(total = regeocode_cache.len(), hits, "Re-geocoded locations missing context");
    }

    Ok((contexts, already_discovered))
}

/// Build hierarchical region names from a context.
fn hierarchical_names(
    city: &Option<String>,
    state: &Option<String>,
    country_name: &Option<String>,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(ref cn) = country_name {
        names.push(cn.clone());
    }
    if let Some(ref st) = state {
        let name = match country_name {
            Some(cn) => format!("{st}, {cn}"),
            None => st.clone(),
        };
        names.push(name);
    }
    if let Some(ref c) = city {
        let name = match state {
            Some(st) => format!("{c}, {st}"),
            None => match country_name {
                Some(cn) => format!("{c}, {cn}"),
                None => c.clone(),
            },
        };
        names.push(name);
    }
    names
}

pub fn plan(ctx: &MigrateContext) -> BoxFuture<Result<String>> {
    let pg = ctx.pg().clone();
    let geocoder = ctx.try_get::<Arc<MapboxGeocoder>>().cloned();
    Box::pin(async move {
        let (contexts, already_discovered) = find_undiscovered_regions(
            &pg,
            geocoder.as_ref().map(|g| g.as_ref()),
        ).await?;

        let mut region_names: HashSet<String> = HashSet::new();
        for ctx in &contexts {
            for name in hierarchical_names(&ctx.city, &ctx.state, &ctx.country_name) {
                if !already_discovered.contains(&name) {
                    region_names.insert(name);
                }
            }
        }

        Ok(format!(
            "{} unique hierarchies → {} regions to discover",
            contexts.len(),
            region_names.len(),
        ))
    })
}

pub fn run(ctx: &MigrateContext) -> BoxFuture<Result<()>> {
    let geocoder = ctx.get::<Arc<MapboxGeocoder>>().cloned();
    let pg = ctx.pg().clone();
    Box::pin(async move {
        let geocoder = geocoder?;

        let (contexts, _) = find_undiscovered_regions(&pg, Some(geocoder.as_ref())).await?;
        if contexts.is_empty() {
            info!("No regions to discover");
            return Ok(());
        }

        let discovered_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT payload->>'name' AS name
             FROM events
             WHERE event_type = 'system:region_discovered'"
        )
        .fetch_all(&pg)
        .await?;
        let mut already_discovered: HashSet<String> = discovered_rows.into_iter().map(|(n,)| n).collect();

        let mut geocode_cache: HashMap<String, Option<(f64, f64)>> = HashMap::new();
        let mut emitted = 0u64;

        for ctx in &contexts {
            struct Candidate {
                name: String,
                search: String,
                scale: &'static str,
                radius_km: f64,
            }

            let mut candidates = Vec::new();

            // Country: name is the country name itself
            if let Some(ref cn) = ctx.country_name {
                candidates.push(Candidate {
                    name: cn.clone(),
                    search: cn.clone(),
                    scale: "country",
                    radius_km: 2500.0,
                });
            }
            // State: "Minnesota, United States"
            if let Some(ref st) = ctx.state {
                let name = match &ctx.country_name {
                    Some(cn) => format!("{st}, {cn}"),
                    None => st.clone(),
                };
                candidates.push(Candidate {
                    search: name.clone(),
                    name,
                    scale: "state",
                    radius_km: 500.0,
                });
            }
            // City: name = "Minneapolis, Minnesota", search = "Minneapolis, Minnesota, United States"
            if let Some(ref c) = ctx.city {
                let name = match &ctx.state {
                    Some(st) => format!("{c}, {st}"),
                    None => match &ctx.country_name {
                        Some(cn) => format!("{c}, {cn}"),
                        None => c.clone(),
                    },
                };
                let search = match (&ctx.state, &ctx.country_name) {
                    (Some(st), Some(cn)) => format!("{c}, {st}, {cn}"),
                    (Some(st), None) => format!("{c}, {st}"),
                    (None, Some(cn)) => format!("{c}, {cn}"),
                    _ => c.clone(),
                };
                candidates.push(Candidate { name, search, scale: "city", radius_km: 25.0 });
            }

            let mut parent_region_id: Option<Uuid> = None;

            for candidate in &candidates {
                if already_discovered.contains(&candidate.name) {
                    parent_region_id = None;
                    continue;
                }

                let coords = if let Some(cached) = geocode_cache.get(&candidate.search) {
                    *cached
                } else {
                    let result = match geocoder.geocode(&candidate.search, None, None).await {
                        Ok(Some(r)) => Some((r.lat, r.lng)),
                        Ok(None) => {
                            tracing::warn!(search = candidate.search.as_str(), "No geocoding result for region center");
                            None
                        }
                        Err(e) => {
                            tracing::warn!(search = candidate.search.as_str(), error = %e, "Geocoding failed for region center");
                            None
                        }
                    };
                    geocode_cache.insert(candidate.search.clone(), result);
                    result
                };

                let (center_lat, center_lng) = match coords {
                    Some(c) => c,
                    None => continue,
                };

                let region_id = Uuid::new_v4();

                let payload = serde_json::json!({
                    "type": "region_discovered",
                    "region_id": region_id,
                    "name": candidate.name,
                    "center_lat": center_lat,
                    "center_lng": center_lng,
                    "radius_km": candidate.radius_km,
                    "city": ctx.city,
                    "state": ctx.state,
                    "country_code": null,
                    "scale": candidate.scale,
                    "parent_region_id": parent_region_id,
                });

                sqlx::query(
                    "INSERT INTO events (event_type, payload, actor, schema_v)
                     VALUES ('system:region_discovered', $1, 'migrate:045', 1)"
                )
                .bind(&payload)
                .execute(&pg)
                .await?;

                info!(name = candidate.name.as_str(), scale = candidate.scale, "Discovered region");

                already_discovered.insert(candidate.name.clone());
                parent_region_id = Some(region_id);
                emitted += 1;
            }
        }

        info!(emitted, "RegionDiscovered events appended");

        sqlx::query("SELECT pg_notify('events', '')")
            .execute(&pg)
            .await?;

        Ok(())
    })
}
