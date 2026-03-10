//! Geocoding lookup — resolve location names to deterministic coordinates.
//!
//! The `GeocodingLookup` trait abstracts geocoding so handlers and migrations
//! can use Mapbox in production and a mock in tests. Results are cached
//! in-memory to avoid redundant API calls within a single process lifetime.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct GeocodingResult {
    pub lat: f64,
    pub lng: f64,
    pub address: Option<String>,
    pub precision: String,
    pub timezone: Option<String>,
}

#[async_trait]
pub trait GeocodingLookup: Send + Sync {
    /// Forward-geocode a location name. Returns None if no match found.
    /// `bias_lat`/`bias_lng` hint the geocoder toward a region (e.g. the signal's known area).
    async fn geocode(
        &self,
        name: &str,
        bias_lat: Option<f64>,
        bias_lng: Option<f64>,
    ) -> Result<Option<GeocodingResult>>;
}

/// Mapbox forward geocoder with in-memory dedup cache.
pub struct MapboxGeocoder {
    token: String,
    client: reqwest::Client,
    cache: Mutex<HashMap<String, Option<GeocodingResult>>>,
}

impl MapboxGeocoder {
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: reqwest::Client::new(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cache_key(name: &str) -> String {
        name.trim().to_lowercase()
    }
}

/// Mapbox Geocoding API v6 response (subset).
#[derive(Deserialize)]
struct MapboxResponse {
    features: Vec<MapboxFeature>,
}

#[derive(Deserialize)]
struct MapboxFeature {
    properties: MapboxProperties,
    geometry: MapboxGeometry,
}

#[derive(Deserialize)]
struct MapboxProperties {
    full_address: Option<String>,
    place_formatted: Option<String>,
    feature_type: Option<String>,
}

#[derive(Deserialize)]
struct MapboxGeometry {
    coordinates: Vec<f64>,
}

fn precision_from_feature_type(ft: Option<&str>) -> &'static str {
    match ft {
        Some("address" | "street" | "poi") => "exact",
        Some("neighborhood" | "postcode" | "locality") => "neighborhood",
        Some("place" | "district") => "approximate",
        Some("region" | "country") => "region",
        _ => "approximate",
    }
}

fn timezone_from_coords(lat: f64, lng: f64) -> Option<String> {
    tz_search::lookup(lat, lng).map(|tz| tz.to_string())
}

/// Test mock: returns pre-configured results by normalized name.
pub struct MockGeocoder {
    results: HashMap<String, GeocodingResult>,
}

impl MockGeocoder {
    pub fn new() -> Self {
        Self { results: HashMap::new() }
    }

    pub fn with_result(mut self, name: &str, result: GeocodingResult) -> Self {
        self.results.insert(name.trim().to_lowercase(), result);
        self
    }
}

#[async_trait]
impl GeocodingLookup for MockGeocoder {
    async fn geocode(
        &self,
        name: &str,
        _bias_lat: Option<f64>,
        _bias_lng: Option<f64>,
    ) -> Result<Option<GeocodingResult>> {
        let key = name.trim().to_lowercase();
        Ok(self.results.get(&key).cloned())
    }
}

#[async_trait]
impl GeocodingLookup for MapboxGeocoder {
    async fn geocode(
        &self,
        name: &str,
        bias_lat: Option<f64>,
        bias_lng: Option<f64>,
    ) -> Result<Option<GeocodingResult>> {
        let key = Self::cache_key(name);

        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(&key) {
                return Ok(cached.clone());
            }
        }

        let mut url = format!(
            "https://api.mapbox.com/search/geocode/v6/forward?q={}&access_token={}&limit=1",
            urlencoding::encode(name.trim()),
            self.token,
        );

        if let (Some(lat), Some(lng)) = (bias_lat, bias_lng) {
            url.push_str(&format!("&proximity={lng},{lat}"));
        }

        debug!(name = name, "Geocoding location via Mapbox");

        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = body, name = name, "Mapbox geocoding failed");
            return Ok(None);
        }

        let body: MapboxResponse = resp.json().await?;

        let result = body.features.first().map(|f| {
            let lng = f.geometry.coordinates[0];
            let lat = f.geometry.coordinates[1];
            let precision = precision_from_feature_type(f.properties.feature_type.as_deref());
            let address = f.properties.full_address.clone()
                .or_else(|| f.properties.place_formatted.clone());
            let timezone = timezone_from_coords(lat, lng);

            GeocodingResult {
                lat,
                lng,
                address,
                precision: precision.to_string(),
                timezone,
            }
        });

        // Cache result (including None for not-found)
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(key, result.clone());
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_maps_poi_to_exact() {
        assert_eq!(precision_from_feature_type(Some("poi")), "exact");
        assert_eq!(precision_from_feature_type(Some("address")), "exact");
        assert_eq!(precision_from_feature_type(Some("street")), "exact");
    }

    #[test]
    fn precision_maps_neighborhood_level() {
        assert_eq!(precision_from_feature_type(Some("neighborhood")), "neighborhood");
        assert_eq!(precision_from_feature_type(Some("postcode")), "neighborhood");
        assert_eq!(precision_from_feature_type(Some("locality")), "neighborhood");
    }

    #[test]
    fn precision_maps_place_to_approximate() {
        assert_eq!(precision_from_feature_type(Some("place")), "approximate");
        assert_eq!(precision_from_feature_type(Some("district")), "approximate");
    }

    #[test]
    fn precision_maps_region_and_country() {
        assert_eq!(precision_from_feature_type(Some("region")), "region");
        assert_eq!(precision_from_feature_type(Some("country")), "region");
    }

    #[test]
    fn precision_defaults_to_approximate_for_unknown() {
        assert_eq!(precision_from_feature_type(None), "approximate");
        assert_eq!(precision_from_feature_type(Some("bogus")), "approximate");
    }

    #[test]
    fn timezone_lookup_returns_iana_for_minneapolis() {
        let tz = timezone_from_coords(44.9778, -93.2650);
        assert_eq!(tz.as_deref(), Some("America/Chicago"));
    }

    #[test]
    fn timezone_lookup_returns_iana_for_london() {
        let tz = timezone_from_coords(51.5074, -0.1278);
        assert_eq!(tz.as_deref(), Some("Europe/London"));
    }

    #[test]
    fn timezone_lookup_returns_iana_for_tokyo() {
        let tz = timezone_from_coords(35.6762, 139.6503);
        assert_eq!(tz.as_deref(), Some("Asia/Tokyo"));
    }

    #[test]
    fn cache_key_normalizes_whitespace_and_case() {
        assert_eq!(MapboxGeocoder::cache_key("  Lake Harriet  "), "lake harriet");
        assert_eq!(MapboxGeocoder::cache_key("MINNEAPOLIS"), "minneapolis");
    }

    #[tokio::test]
    async fn mock_geocoder_returns_configured_result() {
        let geocoder = MockGeocoder::new()
            .with_result("lake harriet bandshell", GeocodingResult {
                lat: 44.9212,
                lng: -93.3090,
                address: Some("Lake Harriet Bandshell, Minneapolis, MN".to_string()),
                precision: "exact".to_string(),
                timezone: Some("America/Chicago".to_string()),
            });

        let result = geocoder.geocode("Lake Harriet Bandshell", None, None).await.unwrap();
        assert!(result.is_some());
        let r = result.unwrap();
        assert!((r.lat - 44.9212).abs() < 0.001);
        assert_eq!(r.precision, "exact");
    }

    #[tokio::test]
    async fn mock_geocoder_returns_none_for_unknown() {
        let geocoder = MockGeocoder::new();
        let result = geocoder.geocode("nonexistent place", None, None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn mapbox_geocoder_caches_results() {
        let geocoder = MapboxGeocoder::new("fake_token".to_string());

        // Pre-populate cache
        {
            let mut cache = geocoder.cache.lock().unwrap();
            cache.insert("minneapolis".to_string(), Some(GeocodingResult {
                lat: 44.9778,
                lng: -93.2650,
                address: Some("Minneapolis, MN".to_string()),
                precision: "approximate".to_string(),
                timezone: Some("America/Chicago".to_string()),
            }));
        }

        // Should return cached result without hitting Mapbox API
        let result = geocoder.geocode("Minneapolis", None, None).await.unwrap();
        assert!(result.is_some());
        assert!((result.unwrap().lat - 44.9778).abs() < 0.001);
    }

    #[tokio::test]
    async fn mapbox_geocoder_caches_none_for_not_found() {
        let geocoder = MapboxGeocoder::new("fake_token".to_string());

        // Pre-populate cache with None
        {
            let mut cache = geocoder.cache.lock().unwrap();
            cache.insert("nowhere".to_string(), None);
        }

        let result = geocoder.geocode("Nowhere", None, None).await.unwrap();
        assert!(result.is_none());
    }
}
