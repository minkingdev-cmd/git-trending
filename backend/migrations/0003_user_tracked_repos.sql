-- backend/migrations/0003_user_tracked_repos.sql
-- Personal repo tracking (logical FKs only — no FOREIGN KEY constraints).
CREATE TABLE user_tracked_repos (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL,
    repo_id     BIGINT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, repo_id)
);
CREATE INDEX idx_user_tracked_user ON user_tracked_repos (user_id);
CREATE INDEX idx_user_tracked_repo ON user_tracked_repos (repo_id);
