-- Fix finished_at column: was NOT NULL DEFAULT now(), which made every row
-- appear "finished" on INSERT. All WHERE finished_at IS NULL checks were dead code.
ALTER TABLE scout_runs ALTER COLUMN finished_at DROP NOT NULL;
ALTER TABLE scout_runs ALTER COLUMN finished_at DROP DEFAULT;
