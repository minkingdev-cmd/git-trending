-- backend/migrations/0002_repo_enrichment.sql
CREATE EXTENSION IF NOT EXISTS pg_trgm;

ALTER TABLE repos
  ADD COLUMN IF NOT EXISTS topics TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS languages JSONB NOT NULL DEFAULT '[]',
  ADD COLUMN IF NOT EXISTS language_names TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS last_enriched_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_repos_topics_gin ON repos USING GIN (topics);
CREATE INDEX IF NOT EXISTS idx_repos_language_names_gin ON repos USING GIN (language_names);
CREATE INDEX IF NOT EXISTS idx_repos_full_name_trgm ON repos USING GIN (full_name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_repos_description_trgm ON repos USING GIN (description gin_trgm_ops);
