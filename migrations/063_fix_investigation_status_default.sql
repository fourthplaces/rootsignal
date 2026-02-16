-- Fix: investigation_status defaulted to 'pending' for all signals,
-- causing every signal to show a misleading "Pending" badge.
-- Default should be NULL (no investigation requested).

ALTER TABLE signals ALTER COLUMN investigation_status SET DEFAULT NULL;

UPDATE signals SET investigation_status = NULL
WHERE needs_investigation = FALSE AND investigation_status = 'pending';
