# Repo Health Signals + Discover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为本地榜单/跟踪补齐健康度字段与徽章筛选，并新增独立「发现」视图（GitHub Search、混合 token、分支限流），方便选型时库外初筛并加入跟踪。

**Architecture:**  
- 模块 A：`repos` 扩展健康列；collector/track enrich 写入；`health()` 纯函数；leaderboard/tracked 查询支持 `exclude_archived` / `active_within`；前端徽章 + 折叠细节。  
- 模块 B：独立 `GET /api/discover/search`（不写 snapshots）；用户 PAT 加密存库优先，否则 `GITHUB_TOKEN`；限流 shared=全局+用户 / user=仅用户；前端第四 tab + Token 设置。

**Tech Stack:** 既有栈 — Rust 2021、axum、SQLx(postgres)、reqwest、chrono、React 19 + Vite + Tailwind 3 + vitest。限流用进程内计数（`std::sync::Mutex` + 时间窗即可，不强制引入 `governor`）。加密用 `aes-gcm` 或项目已有密码学依赖；若无则加 workspace dep `aes-gcm` + `rand`（已有）。

**Spec:** `docs/superpowers/specs/2026-08-07-repo-health-and-discover-design.md`

## Global Constraints

- 本地 PG only（`CLAUDE.md`：禁止 docker 起 PG）；`sqlx::query!` + 提交 `.sqlx/` 离线缓存。
- 测试永不打真实 GitHub；discover/enrich 用 wiremock。
- 发现 **不** 写 `repos`/`snapshots`（仅 track 写业务表）；`board=discover` 不是后端 `Board` 枚举。
- `STALE_AFTER_DAYS = 90`；恰好 90 天算 `active`（用 `duration > 90 days`）。
- 默认 `exclude_archived=1`（API 与前端缺省一致）；`active_within` 默认不传。
- 限流默认：shared 全局 20/min + 用户 10/min；user 路径仅用户 25/min；env 可调。
- Collector **禁止**使用用户 PAT。
- 提交 conventional commits；`export RUSTUP_TOOLCHAIN=stable` 跑测。
- 前端不引入 Ant Design / react-router。

---

## File Structure

```
backend/migrations/
  0005_repo_health.sql              # pushed_at, archived, open_issues_count, created_at_gh, latest_release_at
  0006_user_github_token.sql        # users.github_token_ciphertext, github_token_set_at
backend/crates/core/src/
  health.rs                         # NEW: HealthStatus, compute_health, STALE_AFTER_DAYS
  crypto.rs                         # NEW: encrypt/decrypt PAT (AES-256-GCM)
  config.rs                         # discover rate limit envs; token encryption key
  models.rs                         # RepoInput/rows/filter health fields
  store.rs                          # upsert health; filter; user token CRUD; full_name exists
  users.rs                          # optional token column helpers if not in store
  lib.rs                            # mod health; mod crypto
backend/crates/collector/src/
  enrich.rs                         # parse health fields; fetch_latest_release
  collect.rs                        # wire enrich writes
backend/crates/api/src/
  rate_limit.rs                     # NEW: global + per-user windows
  routes_discover.rs                # NEW: search + q builder
  routes_me_github.rs               # NEW or fold into auth: PUT/DELETE github-token
  routes_leaderboard.rs             # filters + health in JSON
  routes_track.rs                   # health on tracked items; write health on lookup/track
  state.rs                          # RateLimiters + http client
  lib.rs                            # merge routers
frontend/src/
  types.ts                          # health fields; Discover*; Me has_github_token
  api.ts                            # discoverSearch; put/delete github token
  urlState.ts                       # board=discover; d* params; exclude_archived; active_within
  health.ts                         # NEW: badge labels (display only; trust API health)
  components/
    HealthBadge.tsx                 # NEW
    Controls.tsx                    # archived / active_within; discover controls branch
    LeaderboardTable.tsx            # badge column
    TrackedPanel.tsx                # badge
    DiscoverPanel.tsx               # NEW
    GithubTokenModal.tsx            # NEW
    Leaderboard.tsx                 # fourth tab; discover fetch
```

---

### Task 1: Migration — repo health columns

**Files:**
- Create: `backend/migrations/0005_repo_health.sql`

**Interfaces:**
- Produces columns on `repos`:
  - `pushed_at TIMESTAMPTZ NULL`
  - `archived BOOLEAN NOT NULL DEFAULT false`
  - `open_issues_count INT NULL`
  - `created_at_gh TIMESTAMPTZ NULL`
  - `latest_release_at TIMESTAMPTZ NULL`
  - index `idx_repos_pushed_at ON repos (pushed_at DESC NULLS LAST)`

- [ ] **Step 1: Write migration**

```sql
-- backend/migrations/0005_repo_health.sql
ALTER TABLE repos
  ADD COLUMN IF NOT EXISTS pushed_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS archived BOOLEAN NOT NULL DEFAULT false,
  ADD COLUMN IF NOT EXISTS open_issues_count INT,
  ADD COLUMN IF NOT EXISTS created_at_gh TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS latest_release_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_repos_pushed_at ON repos (pushed_at DESC NULLS LAST);
```

- [ ] **Step 2: Apply to local DB**

```bash
set -a && source .env && set +a
# API or collector startup auto-migrates; or:
cd backend && sqlx migrate run
```

Expected: migrate succeeds on `ghtrending`.

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/0005_repo_health.sql
git commit -m "feat(db): add repo health signal columns"
```

---

### Task 2: `health` pure function (core)

**Files:**
- Create: `backend/crates/core/src/health.rs`
- Modify: `backend/crates/core/src/lib.rs` — `pub mod health;`

**Interfaces:**
- Produces:
  - `pub const STALE_AFTER_DAYS: i64 = 90;`
  - `pub enum HealthStatus { Active, Stale, Archived, Unknown }` with `as_str()` → `active|stale|archived|unknown`
  - `pub fn compute_health(archived: bool, pushed_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> HealthStatus`

**Rules (spec):**
1. `archived` → Archived  
2. else `pushed_at is None` → Unknown  
3. else if `now - pushed_at > 90 days` → Stale  
4. else → Active  

- [ ] **Step 1: Write failing unit tests in `health.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn archived_wins() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 0, 0, 0).unwrap();
        let push = now - Duration::days(1);
        assert_eq!(compute_health(true, Some(push), now), HealthStatus::Archived);
    }

    #[test]
    fn null_push_unknown() {
        let now = Utc::now();
        assert_eq!(compute_health(false, None, now), HealthStatus::Unknown);
    }

    #[test]
    fn exactly_90_days_is_active() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap();
        let push = now - Duration::days(90);
        assert_eq!(compute_health(false, Some(push), now), HealthStatus::Active);
    }

    #[test]
    fn over_90_days_stale() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap();
        let push = now - Duration::days(90) - Duration::seconds(1);
        assert_eq!(compute_health(false, Some(push), now), HealthStatus::Stale);
    }
}
```

- [ ] **Step 2: Run tests — expect FAIL (module missing)**

```bash
export RUSTUP_TOOLCHAIN=stable
cd backend && cargo test -p ght-core health::
```

- [ ] **Step 3: Implement `health.rs` to pass**

```rust
use chrono::{DateTime, Duration, Utc};

pub const STALE_AFTER_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Active,
    Stale,
    Archived,
    Unknown,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HealthStatus::Active => "active",
            HealthStatus::Stale => "stale",
            HealthStatus::Archived => "archived",
            HealthStatus::Unknown => "unknown",
        }
    }
}

pub fn compute_health(
    archived: bool,
    pushed_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> HealthStatus {
    if archived {
        return HealthStatus::Archived;
    }
    let Some(pushed) = pushed_at else {
        return HealthStatus::Unknown;
    };
    if now.signed_duration_since(pushed) > Duration::days(STALE_AFTER_DAYS) {
        HealthStatus::Stale
    } else {
        HealthStatus::Active
    }
}
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cd backend && cargo test -p ght-core health::
```

- [ ] **Step 5: Commit**

```bash
git add backend/crates/core/src/health.rs backend/crates/core/src/lib.rs
git commit -m "feat(core): add repo health status pure function"
```

---

### Task 3: Models + store — persist health + leaderboard filters

**Files:**
- Modify: `backend/crates/core/src/models.rs` — `RepoInput`, `LeaderboardFilter`, `LeaderboardRow`, `TrackedRow`
- Modify: `backend/crates/core/src/store.rs` — `upsert_repo`, list queries, tests
- Run: `cargo sqlx prepare` after SQL changes (or offline refresh in CI env)

**Interfaces:**
- `RepoInput` adds:
  - `pushed_at: Option<DateTime<Utc>>`
  - `archived: bool` (default false)
  - `open_issues_count: Option<i32>`
  - `created_at_gh: Option<DateTime<Utc>>`
  - `latest_release_at: Option<DateTime<Utc>>`
- Empty-health upsert semantics: if all health fields “unset” (`pushed_at` none AND `open_issues_count` none AND `created_at_gh` none AND `latest_release_at` none AND `archived == false`), **do not wipe** existing health columns (mirror topics empty-preserve pattern). If any health field is “provided” (e.g. `pushed_at` some OR explicit archived true OR open_issues some), overwrite health columns from input.
- Simpler v1 rule (recommended): always write health columns from `RepoInput` when called from enrich/details paths; board-only upserts use a helper `RepoInput` that copies previous or use dedicated `update_repo_health(...)`.  
  **Implement dedicated:**

```rust
pub async fn update_repo_health(
    pool: &PgPool,
    repo_id: i64,
    pushed_at: Option<DateTime<Utc>>,
    archived: bool,
    open_issues_count: Option<i32>,
    created_at_gh: Option<DateTime<Utc>>,
    latest_release_at: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error>
```

  and optionally extend `upsert_repo` to set health when `pushed_at.is_some() || archived || open_issues_count.is_some()`.

- `LeaderboardFilter` adds:
  - `exclude_archived: bool` — when true, `AND repos.archived = false`
  - `active_within_days: Option<i32>` — when `Some(n)`, `AND pushed_at >= now() - n days AND archived = false`

- `LeaderboardRow` / `TrackedRow` add the five health columns (types matching DB).

- [ ] **Step 1: Extend filter + failing store test**

Add test: insert two repos (one archived, one active push); `exclude_archived=true` returns 1; `active_within_days=90` excludes old push.

- [ ] **Step 2: Run test — FAIL**

```bash
cd backend && cargo test -p ght-core store::tests::exclude_archived -- --nocapture
```

- [ ] **Step 3: Implement migration-aware SQL in all leaderboard SELECT lists + WHERE**

Pattern for WHERE fragments:

```sql
AND ($exclude_archived::bool IS NOT TRUE OR r.archived = false)
AND (
  $active_within::int IS NULL
  OR (r.archived = false AND r.pushed_at IS NOT NULL
      AND r.pushed_at >= (now() - ($active_within::int || ' days')::interval))
)
```

Bind `exclude_archived: bool`, `active_within: Option<i32>`.

Update every list path: trending, top, tracked list.

- [ ] **Step 4: `update_repo_health` + tests for write/read**

- [ ] **Step 5: sqlx offline**

```bash
export DATABASE_URL=postgres://postgres@localhost:5432/ghtrending
cd backend && cargo sqlx prepare --workspace
```

- [ ] **Step 6: All core store tests pass; commit**

```bash
git add backend/crates/core backend/.sqlx
git commit -m "feat(core): store health fields and leaderboard health filters"
```

---

### Task 4: Collector enrich — fill health from GitHub

**Files:**
- Modify: `backend/crates/collector/src/enrich.rs`
- Modify: `backend/crates/collector/src/collect.rs` (call `update_repo_health` / pass fields)
- Test: unit parse tests + wiremock for release 200/404

**Interfaces:**
- `RepoDetails` / `RepoMetaJson` add: `pushed_at`, `archived`, `open_issues_count`, `created_at` (map to `created_at_gh`)
- `pub async fn fetch_latest_release(...) -> Result<Option<DateTime<Utc>>>`
  - 404 → `Ok(None)`
  - 200 → parse `published_at`
  - other errors → `Err` (caller logs and **skips overwriting** `latest_release_at`)

- [ ] **Step 1: Extend JSON structs and map fields in `fetch_repo_details`**

GitHub fields: `pushed_at`, `archived`, `open_issues_count`, `created_at` (RFC3339).

- [ ] **Step 2: Implement `fetch_latest_release`**

```rust
// GET {base}/repos/{owner}/{name}/releases/latest
// 404 -> None
// body.published_at -> DateTime<Utc>
```

- [ ] **Step 3: In `enrich_today_repos` after languages/topics, call health update**

```text
details = fetch_repo_details(...)
release = match fetch_latest_release(...) {
  Ok(v) => v,
  Err(e) => { warn; keep previous latest_release_at via update that passes None with a flag OR read-modify }
}
```

v1 simplification: `update_repo_health` always sets `latest_release_at` from `Option`; on release fetch **error**, call update with only other fields using a variant:

```rust
pub async fn update_repo_health_partial(
  ...
  latest_release_at: Option<Option<DateTime<Utc>>>, // None=don't touch, Some(None)=clear, Some(Some)=set
)
```

Or two SQL paths. Pick one and test.

- [ ] **Step 4: wiremock tests for details + release**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(collector): enrich repo health and latest release"
```

---

### Task 5: API leaderboard + track — expose health

**Files:**
- Modify: `backend/crates/api/src/routes_leaderboard.rs`
- Modify: `backend/crates/api/src/routes_track.rs`

**Interfaces:**
- Query params: `exclude_archived` (`1`/`0`/`true`/`false`; **default true**), `active_within` (optional u32 days)
- Item DTO fields: `pushed_at`, `archived`, `open_issues_count`, `created_at_gh`, `latest_release_at`, `health` (string via `compute_health(..., Utc::now())`)
- Track lookup/track path: after `fetch_repo_details`, write health columns

- [ ] **Step 1: Failing API test — default excludes archived repo from trending list**

- [ ] **Step 2: Implement param parse**

```rust
fn parse_exclude_archived(s: Option<&str>) -> bool {
    match s.map(str::trim) {
        None => true, // default on
        Some("0") | Some("false") | Some("no") => false,
        _ => true,
    }
}
```

- [ ] **Step 3: Map rows → JSON with health**

- [ ] **Step 4: Track path writes health via store**

- [ ] **Step 5: `cargo test -p ght-api` pass; commit**

```bash
git commit -m "feat(api): expose health fields and filters on leaderboard and track"
```

---

### Task 6: Frontend — health badge + board filters

**Files:**
- Create: `frontend/src/components/HealthBadge.tsx`
- Create: `frontend/src/health.ts` (optional labels)
- Modify: `frontend/src/types.ts`, `urlState.ts`, `urlState.test.ts`
- Modify: `frontend/src/components/Controls.tsx`, `LeaderboardTable.tsx`, `TrackedPanel.tsx`, `Leaderboard.tsx`, `api.ts`

**Interfaces:**
- `UrlState` adds: `excludeArchived: boolean` (default `true`), `activeWithin: number | null` (default `null`)
- URL: `exclude_archived=0` to disable; `active_within=90` when set
- `LeaderboardItem` / `TrackedRepoItem` health fields
- `HealthBadge`: shows label by `health`; title/tooltip with push/issues/release/age

- [ ] **Step 1: urlState tests for new params**

- [ ] **Step 2: Implement parse/serialize**

- [ ] **Step 3: HealthBadge component**

```tsx
// Props: health, pushed_at, open_issues_count, latest_release_at, created_at_gh, archived
// Visual: small pill active=green stale=amber archived=gray unknown=muted
// detail: title attribute multi-line or hover panel
```

- [ ] **Step 4: Wire Controls toggles「排除已归档」「仅活跃(90天)」**

- [ ] **Step 5: Table + Tracked show badge; api query string includes params**

- [ ] **Step 6: `cd frontend && npm test` pass; commit**

```bash
git commit -m "feat(web): health badges and archived/active filters"
```

---

### Task 7: Migration + crypto — user GitHub PAT storage

**Files:**
- Create: `backend/migrations/0006_user_github_token.sql`
- Create: `backend/crates/core/src/crypto.rs`
- Modify: `backend/crates/core/src/lib.rs`, `config.rs`, `Cargo.toml` (aes-gcm if needed)
- Modify: `backend/crates/core/src/store.rs` or `users.rs` — get/set/clear ciphertext

**Interfaces:**
- Columns: `github_token_ciphertext BYTEA NULL`, `github_token_set_at TIMESTAMPTZ NULL`
- `Settings`: `token_encryption_key: [u8; 32]` from `TOKEN_ENCRYPTION_KEY` (32-byte base64/hex) **or** SHA-256(`JWT_SECRET` + fixed domain sep `"ght-github-token-v1"`) so local dev works without new env
- `encrypt_token(key, plaintext) -> Vec<u8>` (nonce || ciphertext)
- `decrypt_token(key, bytes) -> String`
- `set_user_github_token(pool, user_id, ciphertext)`
- `clear_user_github_token(pool, user_id)`
- `get_user_github_token_ciphertext(pool, user_id) -> Option<Vec<u8>>`
- `user_has_github_token(pool, user_id) -> bool`

- [ ] **Step 1: Migration**

```sql
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS github_token_ciphertext BYTEA,
  ADD COLUMN IF NOT EXISTS github_token_set_at TIMESTAMPTZ;
```

- [ ] **Step 2: crypto roundtrip unit test**

- [ ] **Step 3: Implement encrypt/decrypt + store helpers**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(core): encrypt and store per-user GitHub tokens"
```

---

### Task 8: Rate limiter module

**Files:**
- Create: `backend/crates/api/src/rate_limit.rs`
- Modify: `backend/crates/api/src/state.rs`, `config`/settings for limits
- Modify: `backend/crates/api/src/lib.rs` — `mod rate_limit`

**Interfaces:**

```rust
pub struct DiscoverRateLimiters { /* mutex state */ }

pub enum RateLimitScope { Global, User }

pub struct RateLimitError {
  pub scope: RateLimitScope,
  pub retry_after_secs: u64,
}

impl DiscoverRateLimiters {
  pub fn from_settings(s: &Settings) -> Self;
  /// shared path: global then per-user
  pub fn check_shared(&self, user_id: i64) -> Result<(), RateLimitError>;
  /// user token path: per-user only (higher limit)
  pub fn check_user_token(&self, user_id: i64) -> Result<(), RateLimitError>;
}
```

Defaults from env via Settings:
- `discover_rate_limit_per_min: u32 = 20`
- `discover_rate_limit_per_user_per_min: u32 = 10`
- `discover_rate_limit_per_user_with_token_per_min: u32 = 25`

Implementation sketch: fixed window or sliding log of timestamps per key `"global"` / `"u:{id}"` / `"ut:{id}"`.

- [ ] **Step 1: Unit tests — 20th shared global ok, 21st err scope Global; user token path does not increment global**

- [ ] **Step 2: Implement + attach to `AppState`**

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(api): discover rate limiters (global and per-user)"
```

---

### Task 9: User PAT HTTP API

**Files:**
- Create: `backend/crates/api/src/routes_me_github.rs` (or extend auth routes)
- Modify: `me` response to include `has_github_token: bool`
- Wire router: `PUT /api/me/github-token`, `DELETE /api/me/github-token`

**Interfaces:**
- PUT body `{ "token": "ghp_..." }` — trim; reject empty; encrypt; store; optional GET `api.github.com/rate_limit` with that token (wiremock in tests); never echo token
- DELETE clears columns
- GET `/api/auth/me` (existing) adds `has_github_token`

- [ ] **Step 1: Integration tests with test user**

- [ ] **Step 2: Implement routes**

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(api): save and clear user GitHub PAT"
```

---

### Task 10: Discover search API

**Files:**
- Create: `backend/crates/api/src/routes_discover.rs`
- Modify: `lib.rs` merge router
- Optional: share search JSON types with collector via small duplicated structs in api (avoid heavy collector dep) 

**Interfaces:**
- `GET /api/discover/search` — RequireAuth
- Query: `q`, `language`, `license`, `min_stars`, `exclude_archived` (default 1), `active_within`, `sort` (`stars|updated`), `page` (1..=10)
- Validate: at least one of non-empty `q`, `language`, `license`, `min_stars`, `active_within`
- Resolve token: decrypt user PAT if set → `auth_mode=user` else settings.github_token → `shared` else 503
- Rate limit by mode
- Build GitHub `q` string; `per_page=30`; call Search API
- Map items; `latest_release_at: null`; `health` via `compute_health`
- Batch: `already_tracked` for user; `in_local_index` via `store::repos_exist_full_names(pool, &names)`
- Errors per spec

**q builder (unit test thoroughly):**

```rust
pub fn build_discover_q(
  user_q: &str,
  language: Option<&str>,
  license: Option<&str>,
  min_stars: Option<u32>,
  exclude_archived: bool,
  active_within_days: Option<u32>,
  now: DateTime<Utc>,
) -> String
```

Example pieces: `language:Rust`, `license:mit`, `stars:>=100`, `archived:false`, `pushed:>YYYY-MM-DD`.

- [ ] **Step 1: Unit tests for `build_discover_q`**

- [ ] **Step 2: wiremock integration — 200 items, 503 no token, 429 rate limit**

Force low limit in test Settings.

- [ ] **Step 3: Implement handler**

- [ ] **Step 4: sqlx prepare if new queries; commit**

```bash
git commit -m "feat(api): discover search via GitHub with hybrid token"
```

---

### Task 11: Frontend — Discover tab + token UI

**Files:**
- Create: `frontend/src/components/DiscoverPanel.tsx`, `GithubTokenModal.tsx`
- Modify: `Leaderboard.tsx`, `Controls.tsx`, `urlState.ts`, `api.ts`, `types.ts`

**Interfaces:**
- `BoardKind` includes `"discover"`
- Discover URL params: `dq`, `dlanguage`, `dlicense`, `dmin_stars`, `dexclude_archived`, `dactive_within`, `dsort`, `dpage`
- On leaving discover, drop `d*` from URL (spec default)
- `discoverSearch(params)` → GET `/api/discover/search`
- `putGithubToken` / `deleteGithubToken`
- UI: fourth board button; discover-only controls; table with track button; show `auth_mode`; 429 message with retry; token modal from header/discover

- [ ] **Step 1: urlState tests for discover board + d* params**

- [ ] **Step 2: API client + types**

- [ ] **Step 3: DiscoverPanel + wire Leaderboard**

- [ ] **Step 4: GithubTokenModal; me.has_github_token**

- [ ] **Step 5: `npm test` + `npm run build`; commit**

```bash
git commit -m "feat(web): discover tab and GitHub token settings"
```

---

### Task 12: Docs + env example + smoke

**Files:**
- Modify: `README.md` — health filters, discover, token envs, rate limits
- Modify: `.env.example` if present

- [ ] **Step 1: Document env vars**

```
DISCOVER_RATE_LIMIT_PER_MIN=20
DISCOVER_RATE_LIMIT_PER_USER_PER_MIN=10
DISCOVER_RATE_LIMIT_PER_USER_WITH_TOKEN_PER_MIN=25
# TOKEN_ENCRYPTION_KEY optional; else derived from JWT_SECRET
GITHUB_TOKEN=   # shared fallback for discover + collector
```

- [ ] **Step 2: Manual smoke checklist in README short section**

1. Leaderboard shows health badge; exclude archived default  
2. Discover without token 503 if no GITHUB_TOKEN  
3. Set PAT → auth_mode=user  
4. Track from discover appears in 我的跟踪  

- [ ] **Step 3: Full test suite**

```bash
make test
# or:
cd backend && cargo test --workspace
cd frontend && npm test && npm run build
```

- [ ] **Step 4: Commit**

```bash
git commit -m "docs: repo health and discover usage"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| G1 health columns + enrich | 1, 3, 4 |
| G2 badges + collapse | 6 |
| G3 exclude_archived / active_within | 3, 5, 6 |
| G4 API health string | 2, 5 |
| G5 discover tab isolated | 10, 11 |
| G6 track from discover | 10, 11 |
| G7 hybrid token | 7, 9, 10 |
| G8 branched rate limit | 8, 10 |
| No discover DB cache | 10 (no cache tables) |
| Collector no user PAT | 4, 10 |
| latest_release discover null | 10 |
| page≤10 per_page=30 | 10 |

## Placeholder / consistency self-review

- No TBD left in tasks.  
- `compute_health` / `HealthStatus::as_str` names consistent across tasks.  
- Rate limit method names `check_shared` / `check_user_token` consistent.  
- URL: board filters use unprefixed `exclude_archived`; discover uses `d*` prefix.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-08-07-repo-health-and-discover.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — this session, batch with checkpoints  

Which approach?
