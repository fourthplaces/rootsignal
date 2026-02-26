CREATE TABLE validation_issues (
    id              UUID            PRIMARY KEY,
    region          TEXT            NOT NULL,
    issue_type      TEXT            NOT NULL,
    severity        TEXT            NOT NULL,
    target_id       UUID            NOT NULL,
    target_label    TEXT            NOT NULL,
    description     TEXT            NOT NULL,
    suggested_action TEXT           NOT NULL,
    status          TEXT            NOT NULL DEFAULT 'open',
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),
    resolved_at     TIMESTAMPTZ,
    resolution      TEXT
);

CREATE INDEX idx_vi_region_status ON validation_issues (region, status);
CREATE INDEX idx_vi_target ON validation_issues (target_id, issue_type) WHERE status = 'open';
