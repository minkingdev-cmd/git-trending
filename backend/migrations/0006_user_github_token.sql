ALTER TABLE users
  ADD COLUMN IF NOT EXISTS github_token_ciphertext BYTEA,
  ADD COLUMN IF NOT EXISTS github_token_set_at TIMESTAMPTZ;
