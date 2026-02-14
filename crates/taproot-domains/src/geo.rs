use std::f64::consts::PI;

const EARTH_RADIUS_MILES: f64 = 3958.8;

/// Haversine distance between two lat/lng points in miles.
pub fn haversine_distance_miles(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let to_rad = |deg: f64| deg * PI / 180.0;

    let dlat = to_rad(lat2 - lat1);
    let dlng = to_rad(lng2 - lng1);

    let a = (dlat / 2.0).sin().powi(2)
        + to_rad(lat1).cos() * to_rad(lat2).cos() * (dlng / 2.0).sin().powi(2);

    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_MILES * c
}

/// Round coordinates to 2 decimal places (~1km precision).
pub fn coarsen_coords(lat: f64, lng: f64) -> (f64, f64) {
    ((lat * 100.0).round() / 100.0, (lng * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minneapolis_to_st_paul() {
        let d = haversine_distance_miles(44.96, -93.27, 44.94, -93.09);
        assert!((d - 9.5).abs() < 1.0, "Expected ~9.5 miles, got {d}");
    }

    #[test]
    fn test_coarsen() {
        let (lat, lng) = coarsen_coords(44.9637, -93.2677);
        assert_eq!(lat, 44.96);
        assert_eq!(lng, -93.27);
    }
}
