-- Remove the Minnesota state default from zip_codes to support multi-region deployments.
ALTER TABLE zip_codes ALTER COLUMN state DROP DEFAULT;
