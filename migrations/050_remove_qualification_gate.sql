-- Sources active by default, qualification columns deprecated
ALTER TABLE sources ALTER COLUMN is_active SET DEFAULT TRUE;

-- Don't drop qualification columns yet — deprecate in code, remove in future migration
COMMENT ON COLUMN sources.qualification_status IS 'DEPRECATED: qualification gate removed. Adaptive cadence handles source quality.';
COMMENT ON COLUMN sources.qualification_score IS 'DEPRECATED: qualification gate removed.';
COMMENT ON COLUMN sources.qualification_summary IS 'DEPRECATED: qualification gate removed.';
