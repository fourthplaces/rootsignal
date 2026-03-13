CREATE TABLE replay_checkpoints (
    projector_name  TEXT        PRIMARY KEY,
    last_seq        BIGINT      NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE supervisor_watermarks (
    region      TEXT        PRIMARY KEY,
    last_run    TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
