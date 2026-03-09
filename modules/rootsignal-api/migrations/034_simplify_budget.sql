-- Replace multi-row budget_configs with single-row budget_config.
-- Two concerns: daily spend ceiling and per-run cap.
DROP TABLE IF EXISTS budget_configs;

CREATE TABLE budget_config (
    id              BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    daily_limit_cents   BIGINT NOT NULL DEFAULT 0,
    per_run_max_cents   BIGINT NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Single row, seeded with unlimited defaults.
INSERT INTO budget_config (daily_limit_cents, per_run_max_cents)
VALUES (0, 0);
