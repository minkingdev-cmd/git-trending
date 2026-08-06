CREATE TABLE repos (
    id            BIGSERIAL PRIMARY KEY,
    full_name     VARCHAR(512) NOT NULL UNIQUE,
    owner         VARCHAR(255) NOT NULL,
    name          VARCHAR(255) NOT NULL,
    html_url      TEXT NOT NULL,
    language      VARCHAR(64),
    description   TEXT,
    first_seen    DATE NOT NULL
);

CREATE TABLE snapshots (
    id             BIGSERIAL PRIMARY KEY,
    repo_id        BIGINT NOT NULL,
    snapshot_date  DATE NOT NULL,
    board          VARCHAR(20) NOT NULL,
    stars          INT NOT NULL,
    forks          INT NOT NULL,
    watchers       INT,
    stars_today    INT,
    UNIQUE (repo_id, snapshot_date, board)
);
CREATE INDEX idx_snapshots_query ON snapshots (snapshot_date, board);
CREATE INDEX idx_snapshots_repo ON snapshots (repo_id);

CREATE TABLE users (
    id                BIGSERIAL PRIMARY KEY,
    username          VARCHAR(64) NOT NULL UNIQUE,
    password_hash     TEXT NOT NULL,
    created_by_invite BIGINT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE invite_codes (
    id          BIGSERIAL PRIMARY KEY,
    code        VARCHAR(32) NOT NULL UNIQUE,
    max_uses    INT NOT NULL DEFAULT 1,
    used_count  INT NOT NULL DEFAULT 0,
    revoked     BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE refresh_tokens (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL,
    token_hash  VARCHAR(64) NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_refresh_tokens_user ON refresh_tokens (user_id);
