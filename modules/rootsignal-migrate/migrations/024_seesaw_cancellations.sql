-- Store-backed cancellation for seesaw engines.
-- Any server can cancel any run by inserting a row here.
-- The settle loop checks is_cancelled() per-correlation_id.
CREATE TABLE IF NOT EXISTS seesaw_cancellations (
    correlation_id UUID PRIMARY KEY,
    cancelled_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
