-- Schema.org field alignment renames.
-- Renames columns and indexes to match Schema.org naming conventions.

-- 1. source_locale → in_language (entities, services, listings)
ALTER TABLE entities RENAME COLUMN source_locale TO in_language;
ALTER TABLE services RENAME COLUMN source_locale TO in_language;
ALTER TABLE listings RENAME COLUMN source_locale TO in_language;

-- 2. ein → tax_id (organizations)
ALTER TABLE organizations RENAME COLUMN ein TO tax_id;

-- 3. phone → telephone (entities, contacts, services)
ALTER TABLE entities RENAME COLUMN phone TO telephone;
ALTER TABLE contacts RENAME COLUMN phone TO telephone;
ALTER TABLE services RENAME COLUMN phone TO telephone;

-- 4. address_line_1 → street_address (locations)
ALTER TABLE locations RENAME COLUMN address_line_1 TO street_address;

-- 5. city → address_locality (locations, zip_codes, service_areas)
ALTER TABLE locations RENAME COLUMN city TO address_locality;
ALTER TABLE zip_codes RENAME COLUMN city TO address_locality;
ALTER TABLE service_areas RENAME COLUMN city TO address_locality;

-- 6. state → address_region (locations, zip_codes, service_areas)
ALTER TABLE locations RENAME COLUMN state TO address_region;
ALTER TABLE zip_codes RENAME COLUMN state TO address_region;
ALTER TABLE service_areas RENAME COLUMN state TO address_region;

-- 7. valid_to → valid_through (schedules)
ALTER TABLE schedules RENAME COLUMN valid_to TO valid_through;

-- 8. content_type → encoding_format (media only)
ALTER TABLE media RENAME COLUMN content_type TO encoding_format;

-- 9. file_size_bytes → content_size (media)
ALTER TABLE media RENAME COLUMN file_size_bytes TO content_size;

-- 10. url → content_url (media only)
ALTER TABLE media RENAME COLUMN url TO content_url;

-- Rename affected indexes for clarity
ALTER INDEX idx_locations_city RENAME TO idx_locations_address_locality;
ALTER INDEX idx_zip_codes_state RENAME TO idx_zip_codes_address_region;
ALTER INDEX idx_zip_codes_city_state RENAME TO idx_zip_codes_locality_region;
ALTER INDEX idx_media_url RENAME TO idx_media_content_url;
