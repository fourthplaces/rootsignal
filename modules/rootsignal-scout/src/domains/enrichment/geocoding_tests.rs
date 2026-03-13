//! Geocoding integration tests — context-aware name resolution.
//!
//! MOCK → ENGINE → OUTPUT: set up MockGeocoder + MockExtractor, run the full
//! pipeline via ScoutRunTest, assert LocationGeocoded events carry correct coordinates.

use std::sync::Arc;

use rootsignal_common::events::SystemEvent;
use rootsignal_common::ScoutScope;
use rootsignal_graph::geocoder::{GeocodingResult, MockGeocoder};

use crate::core::extractor::ExtractionResult;
use crate::testing::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn minnesota_scope() -> ScoutScope {
    ScoutScope {
        center_lat: 44.9778,
        center_lng: -93.2650,
        radius_km: 50.0,
        name: "Minnesota".to_string(),
    }
}

fn extraction_with_locations(nodes: Vec<rootsignal_common::Node>) -> ExtractionResult {
    ExtractionResult {
        nodes,
        raw_signal_count: 1,
        ..Default::default()
    }
}

fn geocoded_events(captured: &[causal::AnyEvent]) -> Vec<(String, f64, f64, Option<String>)> {
    captured.iter().filter_map(|e| {
        if let Some(SystemEvent::LocationGeocoded { location_name, lat, lng, timezone, .. }) =
            e.downcast_ref::<SystemEvent>()
        {
            Some((location_name.clone(), *lat, *lng, timezone.clone()))
        } else {
            None
        }
    }).collect()
}

// ---------------------------------------------------------------------------
// Test 6: Rochester disambiguated by region context
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rochester_disambiguated_by_region_context() {
    let url = "https://rochesterorg.org/events";

    let geocoder = Arc::new(
        MockGeocoder::new()
            .with_result("rochester, minnesota", GeocodingResult {
                lat: 44.02,
                lng: -92.47,
                address: Some("Rochester, MN".to_string()),
                precision: "approximate".to_string(),
                timezone: Some("America/Chicago".to_string()),
                city: None, state: None, country_code: None, country_name: None,
            })
    );

    let harness = ScoutRunTest::new()
        .region(minnesota_scope())
        .source(url, archived_page(url, "Community dinner in Rochester"))
        .extraction(url, extraction_with_locations(vec![
            tension_with_location("Rochester Community Dinner", "Rochester"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(!events.is_empty(), "should emit LocationGeocoded");
    let (name, lat, _lng, tz) = &events[0];
    assert_eq!(name, "Rochester");
    assert!((lat - 44.02).abs() < 0.1, "should geocode to Rochester MN, not NY");
    assert_eq!(tz.as_deref(), Some("America/Chicago"));
}

// ---------------------------------------------------------------------------
// Test 9: No context still geocodes best-effort
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_context_still_geocodes_best_effort() {
    let url = "https://example.org/page";

    let geocoder = Arc::new(
        MockGeocoder::new()
            .with_result("minneapolis city hall", GeocodingResult {
                lat: 44.977,
                lng: -93.265,
                address: Some("Minneapolis City Hall, Minneapolis, MN".to_string()),
                precision: "exact".to_string(),
                timezone: Some("America/Chicago".to_string()),
                city: None, state: None, country_code: None, country_name: None,
            })
    );

    let harness = ScoutRunTest::new()
        .source(url, archived_page(url, "Event at Minneapolis City Hall"))
        .extraction(url, extraction_with_locations(vec![
            tension_with_location("Legal Clinic", "Minneapolis City Hall"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(!events.is_empty(), "should geocode even without region context");
    assert!((events[0].1 - 44.977).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// Test 10: Qualified name from extractor not double-qualified
// ---------------------------------------------------------------------------

#[tokio::test]
async fn qualified_name_from_extractor_not_double_qualified() {
    let url = "https://rochesterorg.org/dinner";

    let geocoder = Arc::new(
        MockGeocoder::new()
            // Only register the un-doubled version — if double-qualified, lookup fails
            .with_result("rochester, minnesota", GeocodingResult {
                lat: 44.02,
                lng: -92.47,
                address: Some("Rochester, MN".to_string()),
                precision: "approximate".to_string(),
                timezone: Some("America/Chicago".to_string()),
                city: None, state: None, country_code: None, country_name: None,
            })
    );

    let harness = ScoutRunTest::new()
        .region(minnesota_scope())
        .source(url, archived_page(url, "Dinner in Rochester, Minnesota"))
        .extraction(url, extraction_with_locations(vec![
            tension_with_location("Community Dinner", "Rochester, Minnesota"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(!events.is_empty(), "should geocode qualified name without doubling");
    assert!((events[0].1 - 44.02).abs() < 0.1);
}

// ---------------------------------------------------------------------------
// Test 11: About-location, not from-location
// ---------------------------------------------------------------------------

#[tokio::test]
async fn about_location_not_from_location() {
    let url = "https://mplsmutualaid.org/news";

    let geocoder = Arc::new(
        MockGeocoder::new()
            .with_result("washington, dc", GeocodingResult {
                lat: 38.90,
                lng: -77.04,
                address: Some("Washington, DC".to_string()),
                precision: "approximate".to_string(),
                timezone: Some("America/New_York".to_string()),
                city: None, state: None, country_code: None, country_name: None,
            })
    );

    let harness = ScoutRunTest::new()
        .region(minnesota_scope())
        .source(url, archived_page(url, "Minneapolis Mutual Aid traveled to Washington DC for a conference"))
        .extraction(url, extraction_with_locations(vec![
            tension_with_location("Food Insecurity Conference", "Washington, DC"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(!events.is_empty(), "should geocode to DC, not Minneapolis");
    assert!((events[0].1 - 38.90).abs() < 0.5, "lat should be DC area");
}

// ---------------------------------------------------------------------------
// Test 13: Italian restaurant on Lake Street
// ---------------------------------------------------------------------------

#[tokio::test]
async fn italian_restaurant_on_lake_street() {
    let url = "https://localfood.org/new";

    let geocoder = Arc::new(
        MockGeocoder::new()
            .with_result("lake street, minneapolis", GeocodingResult {
                lat: 44.948,
                lng: -93.262,
                address: Some("Lake Street, Minneapolis, MN".to_string()),
                precision: "exact".to_string(),
                timezone: Some("America/Chicago".to_string()),
                city: None, state: None, country_code: None, country_name: None,
            })
    );

    let harness = ScoutRunTest::new()
        .region(minnesota_scope())
        .source(url, archived_page(url, "Authentic Italian restaurant now open on Lake Street"))
        .extraction(url, extraction_with_locations(vec![
            tension_with_location("New Italian Restaurant", "Lake Street, Minneapolis"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(!events.is_empty(), "should geocode to Lake Street Minneapolis, not Italy");
    assert!((events[0].1 - 44.948).abs() < 0.1);
}

// ---------------------------------------------------------------------------
// Test 15: Somali family in Phillips
// ---------------------------------------------------------------------------

#[tokio::test]
async fn somali_family_in_phillips() {
    let url = "https://phillipsnews.org/story";

    let geocoder = Arc::new(
        MockGeocoder::new()
            .with_result("phillips, minneapolis", GeocodingResult {
                lat: 44.952,
                lng: -93.261,
                address: Some("Phillips, Minneapolis, MN".to_string()),
                precision: "neighborhood".to_string(),
                timezone: Some("America/Chicago".to_string()),
                city: None, state: None, country_code: None, country_name: None,
            })
    );

    let harness = ScoutRunTest::new()
        .region(minnesota_scope())
        .source(url, archived_page(url, "A Somali family who fled Mogadishu opened a grocery in Phillips"))
        .extraction(url, extraction_with_locations(vec![
            tension_with_location("New Grocery Store", "Phillips, Minneapolis"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(!events.is_empty(), "should geocode to Phillips Minneapolis, not Somalia");
    assert!((events[0].1 - 44.952).abs() < 0.1);
}

// ---------------------------------------------------------------------------
// Test 16: Social post with no specific location omits geocoding
// ---------------------------------------------------------------------------

#[tokio::test]
async fn geographically_neutral_signal_not_geocoded() {
    let url = "https://example.org/post";

    let geocoder = Arc::new(MockGeocoder::new());

    let harness = ScoutRunTest::new()
        .region(minnesota_scope())
        .source(url, archived_page(url, "Cleanup this Saturday at the park!"))
        .extraction(url, extraction_with_locations(vec![
            // No location — geographically neutral signal
            tension("Weekend Park Cleanup"),
        ]))
        .geocoder(geocoder)
        .build();

    harness.run().await;

    let events = geocoded_events(&harness.captured());
    assert!(events.is_empty(), "geographically neutral signal should not be geocoded");
}
