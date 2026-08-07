-- SPDX id or short license key (MIT, Apache-2.0, …); null if unknown / none.
ALTER TABLE repos
  ADD COLUMN IF NOT EXISTS license VARCHAR(64);
