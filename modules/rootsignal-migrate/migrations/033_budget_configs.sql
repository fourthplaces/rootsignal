CREATE TABLE budget_configs (
    scope       TEXT NOT NULL,
    scope_id    TEXT NOT NULL DEFAULT '',
    daily_limit_cents BIGINT NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (scope, scope_id)
);

INSERT INTO budget_configs (scope, daily_limit_cents)
VALUES ('global', 0), ('run_default', 0);

ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS spent_cents BIGINT DEFAULT 0;
