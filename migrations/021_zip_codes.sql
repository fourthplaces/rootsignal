-- Zip code reference table for geo lookups (no PostGIS needed)
CREATE TABLE zip_codes (
    zip_code TEXT PRIMARY KEY,
    city TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'MN',
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL
);

CREATE INDEX idx_zip_codes_state ON zip_codes(state);
CREATE INDEX idx_zip_codes_city_state ON zip_codes(city, state);
CREATE INDEX idx_zip_codes_lat_lng ON zip_codes(latitude, longitude);

-- Haversine distance in miles, safe against acos domain errors
CREATE OR REPLACE FUNCTION haversine_distance(
    lat1 DOUBLE PRECISION,
    lng1 DOUBLE PRECISION,
    lat2 DOUBLE PRECISION,
    lng2 DOUBLE PRECISION
) RETURNS DOUBLE PRECISION
LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT 3958.8 * acos(
        LEAST(1.0, GREATEST(-1.0,
            cos(radians(lat1)) * cos(radians(lat2)) *
            cos(radians(lng2) - radians(lng1)) +
            sin(radians(lat1)) * sin(radians(lat2))
        ))
    )
$$;

-- Drop denormalized lat/lng from listings (always NULL, geo lives in locations via locationables)
ALTER TABLE listings DROP COLUMN IF EXISTS latitude;
ALTER TABLE listings DROP COLUMN IF EXISTS longitude;
DROP INDEX IF EXISTS idx_listings_geo;
