CREATE TABLE budget_configs (
    scope       TEXT NOT NULL,
    scope_id    TEXT,
    daily_limit_cents BIGINT NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (scope, COALESCE(scope_id, ''))
);

INSERT INTO budget_configs (scope, scope_id, daily_limit_cents)
VALUES ('global', NULL, 0), ('run_default', NULL, 0);

ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS spent_cents BIGINT DEFAULT 0;
