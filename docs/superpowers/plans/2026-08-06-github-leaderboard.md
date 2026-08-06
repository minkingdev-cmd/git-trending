# GH Trending 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建一个需登录（邀请码注册）的 GitHub 排行榜站点：collector 每日抓取趋势榜与 star/fork/watch 总榜落库，axum API 提供只读查询，React 前端展示并支持按语言筛选、点击跳转 GitHub。

**Architecture:** Rust cargo workspace 四个 crate：`ght-core`（共享配置/DB/模型/数据访问）、`ght-collector`（抓取 worker，常驻调度或 `--once`）、`ght-api`（axum 无状态 JWT 服务）、`ght-admin`（管理 CLI）。认证：access JWT 自验证（请求路径零查库）+ refresh token（SHA-256 存库、可轮换、低频路径才查）。数据库无外键，只有索引。前端 React+Vite+Tailwind 单页。

**Tech Stack:** Rust 2021、axum、tokio、SQLx(postgres)、reqwest、scraper、jsonwebtoken、rand、sha2、bcrypt、clap、tokio-cron-scheduler、wiremock；React 18 + Vite + Tailwind 3 + vitest。

## Global Constraints

- Rust edition 一律 `2021`；crate 名 `ght-core` / `ght-collector` / `ght-api` / `ght-admin`，目录 `backend/crates/<name 去掉 ght- 前缀>`。
- **SQLx 编译期依赖数据库**：含 `sqlx::query!` 的代码编译前必须 `make db` 且 `DATABASE_URL` 指向运行中的 PG（本计划全部使用 `query!`/`query_as!` 宏）。
- 测试数据库：`DATABASE_URL_TEST`，默认 `postgres://ght:ght@localhost:5433/ghtrending_test`。
- **无外键**：迁移文件禁止 `REFERENCES` / `FOREIGN KEY` / `ON DELETE`；跨表引用列建普通索引。
- 认证常量：access JWT 15 分钟；refresh 30 天；前端定时刷新 10 分钟。
- Cookie：`access_token` 为 `HttpOnly; SameSite=Lax; Path=/`；`refresh_token` 为 `HttpOnly; SameSite=Lax; Path=/api/auth`；`COOKIE_SECURE=true` 时追加 `Secure`。
- API 端口 8000；Vite dev 端口 5173，代理 `/api` → `localhost:8000`。
- 抓取限速：Search API 请求间隔 ≥1.5s，trending 页面 ≥2s；HTTP client 必须设 User-Agent。
- 测试永不访问真实 GitHub，一律 wiremock mock。
- 提交信息用 conventional commits（`feat:` / `test:` / `chore:` / `docs:`）。
- 前端不引入 UI 组件库、不引入 react-router。

---

## File Structure

```
gh-trending/
├── backend/
│   ├── Cargo.toml                  # workspace + workspace.dependencies
│   ├── migrations/
│   │   └── 0001_init.sql           # 5 张表 + 索引（无外键）
│   └── crates/
│       ├── core/                   # ght-core：共享层
│       │   └── src/{lib.rs, config.rs, db.rs, models.rs, store.rs, users.rs, refresh.rs}
│       ├── collector/              # ght-collector：抓取 worker
│       │   ├── src/{main.rs, collect.rs, store.rs, trending.rs, search.rs, graphql.rs}
│       │   └── tests/fixtures/trending.html
│       ├── api/                    # ght-api：axum 服务（lib.rs + main.rs）
│       │   └── src/{lib.rs, main.rs, state.rs, routes_leaderboard.rs,
│       │              auth/{mod.rs, tokens.rs, passwords.rs, cookies.rs, extract.rs, routes.rs}}
│       └── admin/                  # ght-admin：CLI
│           └── src/main.rs
├── frontend/                       # React + Vite + Tailwind
│   └── src/{main.tsx, App.tsx, api.ts, types.ts,
│             components/{AuthCard.tsx, Leaderboard.tsx, Controls.tsx, LeaderboardTable.tsx}}
├── docker-compose.yml              # postgres（含 test 库初始化）
├── dev/init-test-db.sql
├── Makefile
├── .env.example
└── README.md
```

职责边界：`ght-core` 不含任何 HTTP/框架代码；collector 只写库；api 只读库（auth 路径除外）；admin 只调用 core。

---

### Task 1: Workspace 脚手架 + 配置 + DB 池 + 本地基础设施

**Files:**
- Create: `backend/Cargo.toml`
- Create: `backend/crates/core/Cargo.toml`、`backend/crates/core/src/lib.rs`、`backend/crates/core/src/config.rs`、`backend/crates/core/src/db.rs`
- Create: `backend/crates/{collector,api,admin}/Cargo.toml` 与占位 `src/main.rs`（collector/api 后续任务替换；api 需 `src/lib.rs` 占位）
- Create: `docker-compose.yml`、`dev/init-test-db.sql`、`Makefile`、`.env.example`
- Test: `backend/crates/core/src/config.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `ght_core::config::{Settings, ConfigError, parse_languages, validate_collect_time}`、`ght_core::db::pg_pool(database_url: &str) -> Result<PgPool, sqlx::Error>`
- `Settings` 字段：`database_url: String, jwt_secret: String, github_token: Option<String>, languages: Vec<String>, collect_time: String, cookie_secure: bool`

- [ ] **Step 1: 创建 workspace 与四个 crate 骨架**

`backend/Cargo.toml`：

```toml
[workspace]
resolver = "2"
members = ["crates/core", "crates/collector", "crates/api", "crates/admin"]

[workspace.dependencies]
tokio = { version = "1.38", features = ["full"] }
axum = "0.7"
tower = { version = "0.4", features = ["util"] }
tower-http = { version = "0.5", features = ["fs", "trace"] }
http = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.7", default-features = false, features = ["runtime-tokio-rustls", "postgres", "macros", "migrate", "chrono"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
scraper = "0.19"
jsonwebtoken = "9"
rand = "0.8"
sha2 = "0.10"
bcrypt = "0.15"
clap = { version = "4", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tokio-cron-scheduler = "0.10"
wiremock = "0.6"
```

`backend/crates/core/Cargo.toml`：

```toml
[package]
name = "ght-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
sqlx = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
```

collector/api/admin 的 Cargo.toml 同上格式（包名 `ght-collector`/`ght-api`/`ght-admin`），依赖先只写各自最小集：collector 与 api 加 `tokio、anyhow、tracing、tracing-subscriber、sqlx、chrono、serde`，api 另加 `axum、tower、tower-http、http、serde_json`，admin 加 `clap、tokio、anyhow`。全部依赖统一用 `{ workspace = true }`。占位 `src/main.rs` 内容为 `fn main() {}`（api 额外建空 `src/lib.rs`）。

- [ ] **Step 2: 写失败测试（config 解析）**

`backend/crates/core/src/lib.rs`：

```rust
pub mod config;
pub mod db;
```

`backend/crates/core/src/config.rs` 先只写测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_languages_trims_and_drops_empty() {
        assert_eq!(parse_languages("Rust, Go , ,Python"), vec!["Rust", "Go", "Python"]);
    }

    #[test]
    fn defaults_applied_when_optional_missing() {
        let s = Settings::from_map(|k| match k {
            "DATABASE_URL" => Some("postgres://x".into()),
            "JWT_SECRET" => Some("secret".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(s.collect_time, "09:00");
        assert!(s.languages.contains(&"Rust".to_string()));
        assert!(!s.cookie_secure);
        assert!(s.github_token.is_none());
    }

    #[test]
    fn missing_required_is_error() {
        assert!(matches!(Settings::from_map(|_| None).unwrap_err(), ConfigError::Missing(_)));
    }

    #[test]
    fn validates_collect_time() {
        assert!(validate_collect_time("09:00").is_ok());
        assert!(validate_collect_time("25:00").is_err());
        assert!(validate_collect_time("9:00").is_err());
        assert!(validate_collect_time("09:60").is_err());
        assert!(validate_collect_time("0900").is_err());
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd backend && cargo test -p ght-core`
Expected: 编译失败，`Settings`/`parse_languages` 等未定义。

- [ ] **Step 4: 实现 config.rs 与 db.rs**

`config.rs` 测试模块之前补上：

```rust
pub const DEFAULT_LANGUAGES: &str =
    "TypeScript,JavaScript,Python,Java,Go,Rust,C,C++,C#,PHP,Ruby,Swift,Kotlin,Shell";

#[derive(Debug, Clone)]
pub struct Settings {
    pub database_url: String,
    pub jwt_secret: String,
    pub github_token: Option<String>,
    pub languages: Vec<String>,
    pub collect_time: String,
    pub cookie_secure: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(String),
    #[error("invalid COLLECT_TIME {0:?}, expected HH:MM")]
    BadCollectTime(String),
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(|k| std::env::var(k).ok())
    }

    pub fn from_map<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, ConfigError> {
        let database_url = get("DATABASE_URL").ok_or_else(|| ConfigError::Missing("DATABASE_URL".into()))?;
        let jwt_secret = get("JWT_SECRET").ok_or_else(|| ConfigError::Missing("JWT_SECRET".into()))?;
        let languages_raw = get("LANGUAGES").unwrap_or_else(|| DEFAULT_LANGUAGES.to_string());
        let collect_time = get("COLLECT_TIME").unwrap_or_else(|| "09:00".to_string());
        validate_collect_time(&collect_time)?;
        Ok(Settings {
            database_url,
            jwt_secret,
            github_token: get("GITHUB_TOKEN").filter(|s| !s.is_empty()),
            languages: parse_languages(&languages_raw),
            collect_time,
            cookie_secure: get("COOKIE_SECURE").map(|v| v == "true").unwrap_or(false),
        })
    }
}

pub fn parse_languages(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn validate_collect_time(t: &str) -> Result<(), ConfigError> {
    let ok = t.len() == 5
        && t.as_bytes()[2] == b':'
        && t[..2].chars().all(|c| c.is_ascii_digit())
        && t[3..].chars().all(|c| c.is_ascii_digit())
        && t[..2].parse::<u32>().map(|h| h < 24).unwrap_or(false)
        && t[3..].parse::<u32>().map(|m| m < 60).unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ConfigError::BadCollectTime(t.to_string()))
    }
}
```

`db.rs`：

```rust
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn pg_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new().max_connections(8).connect(database_url).await
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-core`
Expected: 4 个测试 PASS。

- [ ] **Step 6: 写基础设施文件**

`docker-compose.yml`：

```yaml
services:
  postgres:
    image: postgres:16
    environment:
      POSTGRES_USER: ght
      POSTGRES_PASSWORD: ght
      POSTGRES_DB: ghtrending
    ports:
      - "5433:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data
      - ./dev/init-test-db.sql:/docker-entrypoint-initdb.d/init-test-db.sql:ro
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ght"]
      interval: 2s
      timeout: 2s
      retries: 10
volumes:
  pgdata:
```

`dev/init-test-db.sql`：

```sql
CREATE DATABASE ghtrending_test;
```

`Makefile`（缩进必须是 TAB）：

```make
.PHONY: db db-down collect dev-all api admin web test

db:
	docker compose up -d postgres
	until docker compose exec -T postgres pg_isready -U ght >/dev/null 2>&1; do sleep 0.5; done

db-down:
	docker compose down

collect:
	cd backend && cargo run -p ght-collector -- --once

dev-all:
	cd backend && cargo run -p ght-collector

api:
	cd backend && cargo run -p ght-api

admin:
	cd backend && cargo run -p ght-admin -- create-user --username admin --password change-me-now

web:
	cd frontend && npm run dev

test:
	cd backend && cargo test
	cd frontend && npm test
```

`.env.example`：

```
DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
DATABASE_URL_TEST=postgres://ght:ght@localhost:5433/ghtrending_test
JWT_SECRET=change-me
GITHUB_TOKEN=
LANGUAGES=
COLLECT_TIME=09:00
COOKIE_SECURE=false
```

- [ ] **Step 7: 验证 workspace 整体编译 + 数据库就绪**

Run: `make db && cd backend && cargo build`
Expected: 编译成功；`docker compose ps` 显示 postgres healthy。

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat: scaffold cargo workspace, settings, db pool, compose/make infra"
```

---

### Task 2: 迁移 + core 数据层（upsert / 榜单查询）

**Files:**
- Create: `backend/migrations/0001_init.sql`
- Create: `backend/crates/core/src/models.rs`、`backend/crates/core/src/store.rs`
- Modify: `backend/crates/core/src/lib.rs`（加模块）、`backend/crates/core/src/db.rs`（加 migrate）
- Test: `store.rs` 内 `#[cfg(test)]`（需要测试库）

**Interfaces:**
- Produces（供 collector/api/admin 使用）：
  - `db::migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError>`
  - `models::{Board, RepoInput, SnapshotInput, LeaderboardRow}`
  - `store::upsert_repo(pool, &RepoInput, NaiveDate) -> Result<i64>`
  - `store::upsert_snapshot(pool, repo_id: i64, NaiveDate, Board, &SnapshotInput) -> Result<()>`
  - `store::top_by_stars / top_by_forks / top_by_watchers(pool, NaiveDate, Option<&str>, i64) -> Result<Vec<LeaderboardRow>>`
  - `store::trending(pool, NaiveDate, Option<&str>, i64) -> Result<Vec<LeaderboardRow>>`
  - `store::latest_snapshot_date(pool, Board) -> Result<Option<NaiveDate>>`
  - `store::languages_with_counts(pool, NaiveDate) -> Result<Vec<(String, i64)>>`
  - `store::cleanup_expired_refresh_tokens(pool) -> Result<u64>`
  - `store::board_count(pool, NaiveDate, Board) -> Result<i64>`

- [ ] **Step 1: 写迁移文件（无外键）**

`backend/migrations/0001_init.sql`：

```sql
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
```

- [ ] **Step 2: db.rs 增加 migrate + lib.rs 挂模块**

`db.rs` 追加：

```rust
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}
```

（路径相对于 `backend/crates/core`，指向 `backend/migrations`。）

`lib.rs`：

```rust
pub mod config;
pub mod db;
pub mod models;
pub mod store;
```

- [ ] **Step 3: 写 models.rs**

```rust
use chrono::NaiveDate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    TrendingDaily,
    TopStars,
    TopForks,
    TopWatchers,
}

impl Board {
    pub fn as_str(self) -> &'static str {
        match self {
            Board::TrendingDaily => "trending_daily",
            Board::TopStars => "top_stars",
            Board::TopForks => "top_forks",
            Board::TopWatchers => "top_watchers",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepoInput {
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub html_url: String,
    pub language: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SnapshotInput {
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct LeaderboardRow {
    pub rank: i64,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}
```

- [ ] **Step 4: 写失败测试（store 行为）**

`store.rs` 先写测试模块（实现留空函数下一步补）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{Board, RepoInput, SnapshotInput};
    use chrono::NaiveDate;
    use sqlx::PgPool;

    pub async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    fn repo(full_name: &str, lang: Option<&str>) -> RepoInput {
        let (owner, name) = full_name.split_once('/').unwrap();
        RepoInput {
            full_name: full_name.into(),
            owner: owner.into(),
            name: name.into(),
            html_url: format!("https://github.com/{full_name}"),
            language: lang.map(String::from),
            description: Some(format!("desc of {full_name}")),
        }
    }

    #[tokio::test]
    async fn upsert_repo_is_idempotent_and_updates_fields() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let id1 = upsert_repo(&pool, &repo("a/x", Some("Python")), date).await.unwrap();
        let id2 = upsert_repo(&pool, &repo("a/x", Some("Go")), date).await.unwrap();
        assert_eq!(id1, id2);
        let langs = languages_with_counts(&pool, date).await.unwrap();
        assert_eq!(langs, vec![("Go".to_string(), 1)]);
    }

    #[tokio::test]
    async fn upsert_snapshot_same_day_same_board_one_row() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let id = upsert_repo(&pool, &repo("a/x", None), date).await.unwrap();
        let snap = SnapshotInput { stars: 10, forks: 1, watchers: None, stars_today: None };
        upsert_snapshot(&pool, id, date, Board::TopStars, &snap).await.unwrap();
        let snap2 = SnapshotInput { stars: 11, forks: 1, watchers: None, stars_today: None };
        upsert_snapshot(&pool, id, date, Board::TopStars, &snap2).await.unwrap();
        let rows = top_by_stars(&pool, date, None, 100).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stars, 11);
    }

    #[tokio::test]
    async fn rank_recomputed_after_language_filter() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (name, lang, stars) in [("a/py1", "Python", 300), ("a/py2", "Python", 100), ("a/rs1", "Rust", 200)] {
            let id = upsert_repo(&pool, &repo(name, Some(lang)), date).await.unwrap();
            upsert_snapshot(&pool, id, date, Board::TopStars, &SnapshotInput { stars, forks: 0, watchers: None, stars_today: None }).await.unwrap();
        }
        let all = top_by_stars(&pool, date, None, 100).await.unwrap();
        assert_eq!(all.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(), vec!["a/py1", "a/rs1", "a/py2"]);
        assert_eq!(all.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 2, 3]);
        let py = top_by_stars(&pool, date, Some("Python"), 100).await.unwrap();
        assert_eq!(py.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(), vec!["a/py1", "a/py2"]);
        assert_eq!(py.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[tokio::test]
    async fn trending_ordered_by_stars_today() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (name, today) in [("a/t1", 5), ("a/t2", 50)] {
            let id = upsert_repo(&pool, &repo(name, None), date).await.unwrap();
            upsert_snapshot(&pool, id, date, Board::TrendingDaily, &SnapshotInput { stars: today, forks: 0, watchers: None, stars_today: Some(today) }).await.unwrap();
        }
        let rows = trending(&pool, date, None, 100).await.unwrap();
        assert_eq!(rows[0].full_name, "a/t2");
        assert_eq!(rows[0].stars_today, Some(50));
    }

    #[tokio::test]
    async fn latest_snapshot_date_returns_max() {
        let pool = test_pool().await;
        let d1 = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        assert_eq!(latest_snapshot_date(&pool, Board::TopStars).await.unwrap(), None);
        for d in [d1, d2] {
            let id = upsert_repo(&pool, &repo("a/x", None), d).await.unwrap();
            upsert_snapshot(&pool, id, d, Board::TopStars, &SnapshotInput { stars: 1, forks: 0, watchers: None, stars_today: None }).await.unwrap();
        }
        assert_eq!(latest_snapshot_date(&pool, Board::TopStars).await.unwrap(), Some(d2));
    }
}
```

- [ ] **Step 5: 运行确认失败（函数未定义）**

Run: `cd backend && cargo test -p ght-core store`
Expected: 编译失败，`upsert_repo` 等未定义。

- [ ] **Step 6: 实现 store.rs**

测试模块之前写实现（`use chrono::NaiveDate; use sqlx::PgPool; use crate::models::{Board, LeaderboardRow, RepoInput, SnapshotInput};`）：

```rust
pub async fn upsert_repo(pool: &PgPool, r: &RepoInput, today: NaiveDate) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"INSERT INTO repos (full_name, owner, name, html_url, language, description, first_seen)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (full_name) DO UPDATE
           SET html_url = EXCLUDED.html_url,
               language = EXCLUDED.language,
               description = EXCLUDED.description
           RETURNING id"#,
        r.full_name,
        r.owner,
        r.name,
        r.html_url,
        r.language,
        r.description,
        today
    )
    .fetch_one(pool)
    .await
}

pub async fn upsert_snapshot(
    pool: &PgPool,
    repo_id: i64,
    date: NaiveDate,
    board: Board,
    s: &SnapshotInput,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO snapshots (repo_id, snapshot_date, board, stars, forks, watchers, stars_today)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (repo_id, snapshot_date, board) DO UPDATE
           SET stars = EXCLUDED.stars,
               forks = EXCLUDED.forks,
               watchers = EXCLUDED.watchers,
               stars_today = EXCLUDED.stars_today"#,
        repo_id,
        date,
        board.as_str(),
        s.stars,
        s.forks,
        s.watchers,
        s.stars_today
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn top_by_stars(pool: &PgPool, date: NaiveDate, language: Option<&str>, limit: i64) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars DESC) AS rank,
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_stars'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.stars DESC
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn top_by_forks(pool: &PgPool, date: NaiveDate, language: Option<&str>, limit: i64) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.forks DESC) AS rank,
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_forks'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.forks DESC
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn top_by_watchers(pool: &PgPool, date: NaiveDate, language: Option<&str>, limit: i64) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.watchers DESC NULLS LAST) AS rank,
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_watchers'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.watchers DESC NULLS LAST
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn trending(pool: &PgPool, date: NaiveDate, language: Option<&str>, limit: i64) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars_today DESC NULLS LAST) AS rank,
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'trending_daily'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.stars_today DESC NULLS LAST
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn latest_snapshot_date(pool: &PgPool, board: Board) -> Result<Option<NaiveDate>, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT MAX(snapshot_date) AS date FROM snapshots WHERE board = $1",
        board.as_str()
    )
    .fetch_one(pool)
    .await?;
    Ok(rec.date)
}

pub async fn languages_with_counts(pool: &PgPool, date: NaiveDate) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT r.language AS lang, COUNT(*) AS cnt
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND r.language IS NOT NULL
           GROUP BY r.language
           ORDER BY cnt DESC"#,
        date
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| Some((r.lang?, r.cnt?)))
        .collect())
}

pub async fn cleanup_expired_refresh_tokens(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query!("DELETE FROM refresh_tokens WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn board_count(pool: &PgPool, date: NaiveDate, board: Board) -> Result<i64, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT COUNT(*) AS cnt FROM snapshots WHERE snapshot_date = $1 AND board = $2",
        date,
        board.as_str()
    )
    .fetch_one(pool)
    .await?;
    Ok(rec.cnt.unwrap_or(0))
}
```

- [ ] **Step 7: 运行测试确认通过**

Run: `make db && cd backend && cargo test -p ght-core`
Expected: 全部 PASS（含 Task 1 的 config 测试）。

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(core): schema migration, repo/snapshot upsert, leaderboard queries"
```

---

### Task 3: trending 页面解析器（fixture 驱动）

**Files:**
- Create: `backend/crates/collector/tests/fixtures/trending.html`
- Create: `backend/crates/collector/src/trending.rs`
- Modify: `backend/crates/collector/src/main.rs`（改为 `mod trending; fn main() {}` 临时占位，Task 7 完成 main）
- Modify: `backend/crates/collector/Cargo.toml`（补齐依赖：`ght-core = { path = "../core" }`、`reqwest、scraper、serde、serde_json、chrono、thiserror、wiremock(dev)、tokio(dev)`）
- Test: `trending.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `trending::{TrendingRepo, parse_trending_html(html: &str) -> Vec<TrendingRepo>, fetch_trending(client: &reqwest::Client, base: &str, lang: Option<&str>) -> anyhow::Result<Vec<TrendingRepo>>}`
- `TrendingRepo { full_name: String, description: Option<String>, language: Option<String>, stars: i32, forks: i32, stars_today: i32 }`

- [ ] **Step 1: 创建 fixture（模拟真实 trending 页面结构）**

`backend/crates/collector/tests/fixtures/trending.html`：

```html
<!DOCTYPE html>
<html>
<body>
<main>
<article class="Box-row">
  <h2 class="h3 lh-condensed">
    <a href="/tensorflow/tensorflow">tensorflow / tensorflow</a>
  </h2>
  <p class="col-9 color-fg-muted my-1 pr-4">An Open Source Machine Learning Framework for Everyone</p>
  <div class="f6 color-fg-muted mt-2">
    <span itemprop="programmingLanguage">Python</span>
    <a class="Link Link--muted d-inline-block mr-3" href="/tensorflow/tensorflow/stargazers">190,000</a>
    <a class="Link Link--muted d-inline-block mr-3" href="/tensorflow/tensorflow/forks">75,000</a>
    <span class="d-inline-block float-sm-right">1,234 stars today</span>
  </div>
</article>
<article class="Box-row">
  <h2 class="h3 lh-condensed">
    <a href="/oven-sh/bun">oven-sh / bun</a>
  </h2>
  <p class="col-9 color-fg-muted my-1 pr-4">Incredibly fast JavaScript runtime, bundler, test runner, and package manager</p>
  <div class="f6 color-fg-muted mt-2">
    <span itemprop="programmingLanguage">Rust</span>
    <a class="Link Link--muted d-inline-block mr-3" href="/oven-sh/bun/stargazers">80,123</a>
    <a class="Link Link--muted d-inline-block mr-3" href="/oven-sh/bun/forks">2,700</a>
    <span class="d-inline-block float-sm-right">801 stars today</span>
  </div>
</article>
<article class="Box-row">
  <h2 class="h3 lh-condensed">
    <a href="/awesome-lists/awesome">awesome-lists / awesome</a>
  </h2>
  <div class="f6 color-fg-muted mt-2">
    <a class="Link Link--muted d-inline-block mr-3" href="/awesome-lists/awesome/stargazers">321,000</a>
    <a class="Link Link--muted d-inline-block mr-3" href="/awesome-lists/awesome/forks">27,000</a>
  </div>
</article>
</main>
</body>
</html>
```

（第三个 article 无描述、无语言、无 stars today——覆盖缺失字段路径。）

- [ ] **Step 2: 写失败测试**

`trending.rs` 先只写测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_rows() {
        let html = include_str!("../tests/fixtures/trending.html");
        let repos = parse_trending_html(html);
        assert_eq!(repos.len(), 3);

        assert_eq!(repos[0].full_name, "tensorflow/tensorflow");
        assert_eq!(repos[0].stars, 190_000);
        assert_eq!(repos[0].forks, 75_000);
        assert_eq!(repos[0].stars_today, 1_234);
        assert_eq!(repos[0].language.as_deref(), Some("Python"));
        assert!(repos[0].description.as_deref().unwrap().starts_with("An Open Source"));

        assert_eq!(repos[1].full_name, "oven-sh/bun");
        assert_eq!(repos[1].stars_today, 801);

        assert_eq!(repos[2].full_name, "awesome-lists/awesome");
        assert_eq!(repos[2].language, None);
        assert_eq!(repos[2].description, None);
        assert_eq!(repos[2].stars_today, 0);
    }

    #[tokio::test]
    async fn fetch_parses_response_and_sends_user_agent() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/trending/rust"))
            .and(query_param("since", "daily"))
            .and(header("user-agent", "gh-trending-collector/0.1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../tests/fixtures/trending.html")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .user_agent("gh-trending-collector/0.1")
            .build()
            .unwrap();
        let repos = fetch_trending(&client, &server.uri(), Some("rust")).await.unwrap();
        assert_eq!(repos.len(), 3);
    }
}
```

- [ ] **Step 3: 运行确认失败**

Run: `cd backend && cargo test -p ght-collector`
Expected: 编译失败，`parse_trending_html` 未定义。

- [ ] **Step 4: 实现解析与抓取**

测试模块之前写：

```rust
use scraper::{Html, Selector};

#[derive(Debug, Clone, PartialEq)]
pub struct TrendingRepo {
    pub full_name: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub stars_today: i32,
}

pub fn parse_trending_html(html: &str) -> Vec<TrendingRepo> {
    let doc = Html::parse_document(html);
    let row_sel = Selector::parse("article.Box-row").unwrap();
    let href_sel = Selector::parse("h2 a").unwrap();
    let desc_sel = Selector::parse("p").unwrap();
    let lang_sel = Selector::parse("[itemprop=programmingLanguage]").unwrap();
    let stars_sel = Selector::parse(r#"a[href$="/stargazers"]"#).unwrap();
    let forks_sel = Selector::parse(r#"a[href$="/forks"]"#).unwrap();
    let today_sel = Selector::parse("span.float-sm-right").unwrap();

    doc.select(&row_sel)
        .filter_map(|row| {
            let href = row.select(&href_sel).next()?.value().attr("href")?;
            let full_name = href.trim_start_matches('/').to_string();
            let stars = parse_count(&row.select(&stars_sel).next()?.text().collect::<String>())?;
            let forks = parse_count(&row.select(&forks_sel).next()?.text().collect::<String>())?;
            let stars_today = row
                .select(&today_sel)
                .next()
                .and_then(|el| parse_count(&el.text().collect::<String>()))
                .unwrap_or(0);
            Some(TrendingRepo {
                full_name,
                description: row
                    .select(&desc_sel)
                    .next()
                    .map(|el| el.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty()),
                language: row
                    .select(&lang_sel)
                    .next()
                    .map(|el| el.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty()),
                stars,
                forks,
                stars_today,
            })
        })
        .collect()
}

fn parse_count(text: &str) -> Option<i32> {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub async fn fetch_trending(
    client: &reqwest::Client,
    base: &str,
    lang: Option<&str>,
) -> anyhow::Result<Vec<TrendingRepo>> {
    let url = match lang {
        Some(l) => format!("{base}/trending/{l}?since=daily"),
        None => format!("{base}/trending?since=daily"),
    };
    let html = client.get(&url).send().await?.error_for_status()?.text().await?;
    Ok(parse_trending_html(&html))
}
```

`main.rs` 临时改为：

```rust
mod trending;

fn main() {}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-collector`
Expected: 2 个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(collector): github trending page parser + fetcher"
```

---

### Task 4: Search API 客户端（总榜 star/fork + watch 候选池）

**Files:**
- Create: `backend/crates/collector/src/search.rs`
- Modify: `backend/crates/collector/src/main.rs`（加 `mod search;`）
- Test: `search.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `search::{Metric, SearchRepo, search_top}`
- `search_top(client: &reqwest::Client, base: &str, token: Option<&str>, lang: Option<&str>, metric: Metric, per_page: u32, pages: u32) -> anyhow::Result<Vec<SearchRepo>>`
- `SearchRepo { full_name, html_url, description: Option<String>, language: Option<String>, stars: i32, forks: i32 }`（字段均为 `pub`）

- [ ] **Step 1: 写失败测试**

`search.rs` 测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn page_body(names: &[&str]) -> String {
        let items: Vec<String> = names
            .iter()
            .map(|n| format!(
                r#"{{"full_name":"{n}","html_url":"https://github.com/{n}","description":"d","language":"Python","stargazers_count":100,"forks_count":10}}"#
            ))
            .collect();
        format!(r#"{{"total_count":{},"items":[{}]}}"#, names.len(), items.join(","))
    }

    #[tokio::test]
    async fn sends_auth_query_params_and_parses_items() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("q", "language:Python"))
            .and(query_param("sort", "stars"))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .and(header("authorization", "Bearer t0k3n"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&["a/x", "b/y"])))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let repos = search_top(&client, &server.uri(), Some("t0k3n"), Some("Python"), Metric::Stars, 100, 1)
            .await
            .unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].full_name, "a/x");
        assert_eq!(repos[0].stars, 100);
        assert_eq!(repos[0].forks, 10);
    }

    #[tokio::test]
    async fn paginates_until_empty_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&["a/x"])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&[])))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let repos = search_top(&client, &server.uri(), None, None, Metric::Forks, 1, 5).await.unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[tokio::test]
    async fn non_2xx_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        assert!(search_top(&client, &server.uri(), None, None, Metric::Stars, 100, 1).await.is_err());
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test -p ght-collector search`
Expected: 编译失败，`search_top` 未定义。

- [ ] **Step 3: 实现 search.rs**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Stars,
    Forks,
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Stars => "stars",
            Metric::Forks => "forks",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchRepo {
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    full_name: String,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    stargazers_count: i32,
    forks_count: i32,
}

pub async fn search_top(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    lang: Option<&str>,
    metric: Metric,
    per_page: u32,
    pages: u32,
) -> anyhow::Result<Vec<SearchRepo>> {
    let mut out = Vec::new();
    let q = match lang {
        Some(l) => format!("language:{l}"),
        None => String::new(),
    };
    for page in 1..=pages {
        let mut req = client
            .get(format!("{base}/search/repositories"))
            .header("Accept", "application/vnd.github+json")
            .query(&[
                ("q", q.as_str()),
                ("sort", metric.as_str()),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ]);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp: SearchResponse = req.send().await?.error_for_status()?.json().await?;
        if resp.items.is_empty() {
            break;
        }
        out.extend(resp.items.into_iter().map(|it| SearchRepo {
            full_name: it.full_name,
            html_url: it.html_url,
            description: it.description,
            language: it.language,
            stars: it.stargazers_count,
            forks: it.forks_count,
        }));
    }
    Ok(out)
}
```

`main.rs` 加 `mod search;`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-collector`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(collector): github search api client with pagination"
```

---

### Task 5: GraphQL 客户端（watch 数批量查询）

**Files:**
- Create: `backend/crates/collector/src/graphql.rs`
- Modify: `backend/crates/collector/src/main.rs`（加 `mod graphql;`）
- Test: `graphql.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `graphql::{WatchTarget, build_watchers_query, parse_watchers, fetch_watchers}`
- `fetch_watchers(client: &reqwest::Client, base: &str, token: &str, targets: &[WatchTarget], batch_size: usize) -> anyhow::Result<std::collections::HashMap<String, i32>>`（key = `owner/name`）
- `WatchTarget { owner: String, name: String }`

- [ ] **Step 1: 写失败测试**

`graphql.rs` 测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn builds_aliased_query() {
        let q = build_watchers_query(&[
            WatchTarget { owner: "a".into(), name: "x".into() },
            WatchTarget { owner: "b".into(), name: "y".into() },
        ]);
        assert!(q.contains(r#"q0: repository(owner: "a", name: "x")"#));
        assert!(q.contains(r#"q1: repository(owner: "b", name: "y")"#));
        assert!(q.starts_with("query {"));
    }

    #[test]
    fn parse_skips_null_repositories() {
        let batch = vec![
            WatchTarget { owner: "a".into(), name: "x".into() },
            WatchTarget { owner: "gone".into(), name: "repo".into() },
        ];
        let resp = json!({"data": {"q0": {"watchers": {"totalCount": 42}}, "q1": null}});
        let parsed = parse_watchers(&resp, &batch);
        assert_eq!(parsed, vec![("a/x".to_string(), 42)]);
    }

    #[tokio::test]
    async fn fetch_batches_and_sends_bearer() {
        let server = MockServer::start().await;
        let body1 = json!({"query": build_watchers_query(&[
            WatchTarget { owner: "a".into(), name: "x".into() },
            WatchTarget { owner: "b".into(), name: "y".into() },
        ])});
        let body2 = json!({"query": build_watchers_query(&[
            WatchTarget { owner: "c".into(), name: "z".into() },
        ])});
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(header("authorization", "Bearer t0k3n"))
            .and(body_json(&body1))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"q0": {"watchers": {"totalCount": 1}}, "q1": {"watchers": {"totalCount": 2}}}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_json(&body2))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"q0": {"watchers": {"totalCount": 3}}}}),
            ))
            .mount(&server)
            .await;

        let targets: Vec<WatchTarget> = ["a/x", "b/y", "c/z"]
            .iter()
            .map(|s| {
                let (o, n) = s.split_once('/').unwrap();
                WatchTarget { owner: (*o).into(), name: (*n).into() }
            })
            .collect();
        let client = reqwest::Client::new();
        let map = fetch_watchers(&client, &server.uri(), "t0k3n", &targets, 2).await.unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map["c/z"], 3);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test -p ght-collector graphql`
Expected: 编译失败。

- [ ] **Step 3: 实现 graphql.rs**

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub owner: String,
    pub name: String,
}

pub fn build_watchers_query(batch: &[WatchTarget]) -> String {
    let fields: Vec<String> = batch
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.owner.contains('"') && !t.name.contains('"'))
        .map(|(i, t)| {
            format!(
                r#"q{i}: repository(owner: "{}", name: "{}") {{ watchers {{ totalCount }} }}"#,
                t.owner, t.name
            )
        })
        .collect();
    format!("query {{ {} }}", fields.join(" "))
}

pub fn parse_watchers(resp: &serde_json::Value, batch: &[WatchTarget]) -> Vec<(String, i32)> {
    let data = resp.get("data").unwrap_or(&serde_json::Value::Null);
    batch
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let count = data
                .get(format!("q{i}"))?
                .get("watchers")?
                .get("totalCount")?
                .as_i64()?;
            Some((format!("{}/{}", t.owner, t.name), count as i32))
        })
        .collect()
}

pub async fn fetch_watchers(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    targets: &[WatchTarget],
    batch_size: usize,
) -> anyhow::Result<HashMap<String, i32>> {
    let mut map = HashMap::new();
    for chunk in targets.chunks(batch_size.max(1)) {
        let body = serde_json::json!({ "query": build_watchers_query(chunk) });
        let resp: serde_json::Value = client
            .post(format!("{base}/graphql"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        map.extend(parse_watchers(&resp, chunk));
    }
    Ok(map)
}
```

`main.rs` 加 `mod graphql;`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-collector`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(collector): graphql batch watchers client for watch board"
```

---

### Task 6: collector 落库 + 编排（collect_once 全流程 + 集成测试）

**Files:**
- Create: `backend/crates/collector/src/store.rs`、`backend/crates/collector/src/collect.rs`
- Modify: `backend/crates/collector/src/main.rs`（加 `mod store; mod collect;`）
- Test: `collect.rs` 内 `#[cfg(test)]`（wiremock + 测试库的集成测试）

**Interfaces:**
- Consumes: Task 2 `ght_core::store::{upsert_repo, upsert_snapshot, cleanup_expired_refresh_tokens}`、`ght_core::models::{Board, RepoInput, SnapshotInput}`；Task 3-5 的 `trending::fetch_trending`、`search::{search_top, Metric}`、`graphql::{fetch_watchers, WatchTarget}`
- Produces:
  - `store::{TopEntry, split_full_name, store_top_rows, store_trending_rows}`
  - `TopEntry { repo_full_name: String, html_url: String, description: Option<String>, language: Option<String>, stars: i32, forks: i32, watchers: Option<i32> }`
  - `collect::{Collector, Report}`；`Collector { pool: PgPool, http: reqwest::Client, settings: Settings, github_base: String, api_base: String }`；`Collector::collect_once(&self) -> Report`；`Report { ok: usize, failed: usize }`

- [ ] **Step 1: 实现 store.rs（含单测）**

```rust
use chrono::NaiveDate;
use ght_core::models::{Board, RepoInput, SnapshotInput};
use ght_core::store as core_store;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct TopEntry {
    pub repo_full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
}

pub fn split_full_name(full_name: &str) -> (&str, &str) {
    full_name.split_once('/').unwrap_or((full_name, ""))
}

async fn store_one(
    pool: &PgPool,
    date: NaiveDate,
    board: Board,
    full_name: &str,
    html_url: &str,
    description: &Option<String>,
    language: &Option<String>,
    snap: SnapshotInput,
) -> anyhow::Result<()> {
    let (owner, name) = split_full_name(full_name);
    let repo = RepoInput {
        full_name: full_name.to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
        html_url: html_url.to_string(),
        language: language.clone(),
        description: description.clone(),
    };
    let repo_id = core_store::upsert_repo(pool, &repo, date).await?;
    core_store::upsert_snapshot(pool, repo_id, date, board, &snap).await?;
    Ok(())
}

pub async fn store_top_rows(pool: &PgPool, date: NaiveDate, board: Board, rows: &[TopEntry]) -> anyhow::Result<usize> {
    let mut n = 0;
    for r in rows {
        store_one(
            pool,
            date,
            board,
            &r.repo_full_name,
            &r.html_url,
            &r.description,
            &r.language,
            SnapshotInput { stars: r.stars, forks: r.forks, watchers: r.watchers, stars_today: None },
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

pub async fn store_trending_rows(pool: &PgPool, date: NaiveDate, rows: &[crate::trending::TrendingRepo]) -> anyhow::Result<usize> {
    let mut n = 0;
    for t in rows {
        store_one(
            pool,
            date,
            Board::TrendingDaily,
            &t.full_name,
            &format!("https://github.com/{}", t.full_name),
            &t.description,
            &t.language,
            SnapshotInput { stars: t.stars, forks: t.forks, watchers: None, stars_today: Some(t.stars_today) },
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_full_name_handles_missing_slash() {
        assert_eq!(split_full_name("a/b"), ("a", "b"));
        assert_eq!(split_full_name("lonely"), ("lonely", ""));
    }
}
```

- [ ] **Step 2: 写 collect.rs 编排**

```rust
use crate::graphql::{fetch_watchers, WatchTarget};
use crate::search::{search_top, Metric};
use crate::store::{split_full_name, store_top_rows, store_trending_rows, TopEntry};
use crate::trending::fetch_trending;
use chrono::Utc;
use ght_core::config::Settings;
use ght_core::models::Board;
use ght_core::store as core_store;
use sqlx::PgPool;
use std::time::Duration;

pub struct Collector {
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub settings: Settings,
    pub github_base: String,
    pub api_base: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub ok: usize,
    pub failed: usize,
}

const SEARCH_INTERVAL: Duration = Duration::from_millis(1500);
const TRENDING_INTERVAL: Duration = Duration::from_millis(2000);

impl Collector {
    fn langs(&self) -> Vec<Option<String>> {
        std::iter::once(None)
            .chain(self.settings.languages.iter().map(|l| Some(l.clone())))
            .collect()
    }

    pub async fn collect_once(&self) -> Report {
        let today = Utc::now().date_naive();
        let mut report = Report::default();
        let token = self.settings.github_token.as_deref();

        // 1. 总榜 stars / forks
        for lang in self.langs() {
            for metric in [Metric::Stars, Metric::Forks] {
                let board = match metric {
                    Metric::Stars => Board::TopStars,
                    Metric::Forks => Board::TopForks,
                };
                match search_top(&self.http, &self.api_base, token, lang.as_deref(), metric, 100, 1).await {
                    Ok(rows) => {
                        let entries: Vec<TopEntry> = rows
                            .into_iter()
                            .map(|r| TopEntry {
                                repo_full_name: r.full_name,
                                html_url: r.html_url,
                                description: r.description,
                                language: r.language,
                                stars: r.stars,
                                forks: r.forks,
                                watchers: None,
                            })
                            .collect();
                        match store_top_rows(&self.pool, today, board, &entries).await {
                            Ok(n) => report.ok += n,
                            Err(e) => {
                                tracing::warn!(error = %e, board = board.as_str(), "store failed");
                                report.failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, lang = ?lang, metric = ?metric, "search failed");
                        report.failed += 1;
                    }
                }
                tokio::time::sleep(SEARCH_INTERVAL).await;
            }
        }

        // 2. 总榜 watchers（候选池 = star top500，需 token）
        match token {
            None => tracing::warn!("GITHUB_TOKEN not set; skipping watch board"),
            Some(token) => {
                for lang in self.langs() {
                    let pool_candidates = search_top(&self.http, &self.api_base, Some(token), lang.as_deref(), Metric::Stars, 100, 5).await;
                    match pool_candidates {
                        Ok(candidates) => {
                            let targets: Vec<WatchTarget> = candidates
                                .iter()
                                .map(|c| {
                                    let (o, n) = split_full_name(&c.full_name);
                                    WatchTarget { owner: o.to_string(), name: n.to_string() }
                                })
                                .collect();
                            match fetch_watchers(&self.http, &self.api_base, token, &targets, 50).await {
                                Ok(watchers) => {
                                    let mut scored: Vec<TopEntry> = candidates
                                        .into_iter()
                                        .filter_map(|c| {
                                            let w = *watchers.get(&c.full_name)?;
                                            Some(TopEntry {
                                                repo_full_name: c.full_name,
                                                html_url: c.html_url,
                                                description: c.description,
                                                language: c.language,
                                                stars: c.stars,
                                                forks: c.forks,
                                                watchers: Some(w),
                                            })
                                        })
                                        .collect();
                                    scored.sort_by(|a, b| b.watchers.unwrap_or(0).cmp(&a.watchers.unwrap_or(0)));
                                    scored.truncate(100);
                                    match store_top_rows(&self.pool, today, Board::TopWatchers, &scored).await {
                                        Ok(n) => report.ok += n,
                                        Err(e) => {
                                            tracing::warn!(error = %e, "watch store failed");
                                            report.failed += 1;
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, lang = ?lang, "graphql failed");
                                    report.failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, lang = ?lang, "watch candidate search failed");
                            report.failed += 1;
                        }
                    }
                    tokio::time::sleep(SEARCH_INTERVAL).await;
                }
            }
        }

        // 3. 趋势榜
        for lang in self.langs() {
            match fetch_trending(&self.http, &self.github_base, lang.as_deref()).await {
                Ok(rows) => match store_trending_rows(&self.pool, today, &rows).await {
                    Ok(n) => report.ok += n,
                    Err(e) => {
                        tracing::warn!(error = %e, "trending store failed");
                        report.failed += 1;
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, lang = ?lang, "trending fetch failed");
                    report.failed += 1;
                }
            }
            tokio::time::sleep(TRENDING_INTERVAL).await;
        }

        // 4. 清理过期 refresh token
        if let Err(e) = core_store::cleanup_expired_refresh_tokens(&self.pool).await {
            tracing::warn!(error = %e, "refresh token cleanup failed");
        }

        report
    }
}
```

- [ ] **Step 3: 写集成测试（wiremock + 测试库）**

`collect.rs` 底部加测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ght_core::{db, store as core_store};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    fn trending_body() -> String {
        include_str!("../tests/fixtures/trending.html").to_string()
    }

    fn search_body(names: &[&str], stars: i32) -> String {
        let items: Vec<String> = names
            .iter()
            .map(|n| format!(
                r#"{{"full_name":"{n}","html_url":"https://github.com/{n}","description":null,"language":"Python","stargazers_count":{stars},"forks_count":3}}"#
            ))
            .collect();
        format!(r#"{{"total_count":{},"items":[{}]}}"#, names.len(), items.join(","))
    }

    async fn mount_all(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_string(search_body(&["a/top"], 999)))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"q0": {"watchers": {"totalCount": 77}}}})))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex("/trending.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(trending_body()))
            .mount(server)
            .await;
    }

    use wiremock::matchers::path_regex;

    fn test_collector(pool: PgPool, uri: String, token: Option<String>) -> Collector {
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some("postgres://unused".into()),
            "JWT_SECRET" => Some("unused".into()),
            "LANGUAGES" => Some("Rust".into()),
            "GITHUB_TOKEN" => token,
            _ => None,
        })
        .unwrap();
        Collector {
            pool,
            http: reqwest::Client::builder().user_agent("gh-trending-collector/0.1").build().unwrap(),
            settings,
            github_base: uri.clone(),
            api_base: uri,
        }
    }

    #[tokio::test]
    async fn collect_once_writes_all_boards_and_is_idempotent() {
        let server = MockServer::start().await;
        mount_all(&server).await;
        let pool = test_pool().await;
        let collector = test_collector(pool.clone(), server.uri(), Some("t0k3n".into()));

        // 集成测试走真实 sleep 太慢：本测试接受 ~14s（(2 langs × 2 metrics + 2 langs watch + 2 langs trending) × 间隔）。
        // 若需加速可将 SEARCH_INTERVAL/TRENDING_INTERVAL 改为可配置字段；此处保持与生产一致。
        let report = collector.collect_once().await;
        assert!(report.failed == 0, "report: ok={} failed={}", report.ok, report.failed);

        let today = Utc::now().date_naive();
        assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
        assert_eq!(core_store::top_by_forks(&pool, today, None, 100).await.unwrap().len(), 1);
        let watch = core_store::top_by_watchers(&pool, today, None, 100).await.unwrap();
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].watchers, Some(77));
        assert_eq!(core_store::trending(&pool, today, None, 100).await.unwrap().len(), 3);

        // 幂等：重跑行数不变
        let report2 = collector.collect_once().await;
        assert!(report2.failed == 0);
        assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_token_skips_watch_board_but_keeps_others() {
        let server = MockServer::start().await;
        mount_all(&server).await;
        let pool = test_pool().await;
        let collector = test_collector(pool.clone(), server.uri(), None);
        let report = collector.collect_once().await;
        assert!(report.failed == 0);
        let today = Utc::now().date_naive();
        assert_eq!(core_store::top_by_watchers(&pool, today, None, 100).await.unwrap().len(), 0);
        assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
    }
}
```

注意：`use wiremock::matchers::path_regex;` 放在模块内任何位置均可编译，若格式工具报警可移到模块顶部。集成测试耗时 ~15s（生产限速间隔），属预期；不得为测试缩短常量。

- [ ] **Step 4: main.rs 挂模块并运行测试**

`main.rs`：

```rust
mod collect;
mod graphql;
mod search;
mod store;
mod trending;

fn main() {}
```

Run: `make db && cd backend && cargo test -p ght-collector -- --nocapture`
Expected: 全部 PASS（含 2 个集成测试，耗时正常）。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(collector): daily collection orchestration with per-language fault isolation"
```

---

### Task 7: collector main + 常驻调度 + config 补充

**Files:**
- Modify: `backend/crates/collector/src/main.rs`（完整实现）
- Modify: `backend/crates/core/src/config.rs`（加 `collect_time_parts`）
- Test: `config.rs` 测试追加

**Interfaces:**
- Consumes: Task 1 `Settings`、Task 6 `Collector`
- Produces: `ght_core::config::collect_time_parts(t: &str) -> Result<(u32, u32), ConfigError>`；collector 二进制支持 `--once`

- [ ] **Step 1: 写失败测试（collect_time_parts）**

`config.rs` tests 模块追加：

```rust
#[test]
fn collect_time_parts_splits_hh_mm() {
    assert_eq!(collect_time_parts("09:05").unwrap(), (9, 5));
    assert!(collect_time_parts("09:60").is_err());
}
```

Run: `cd backend && cargo test -p ght-core collect_time_parts`
Expected: FAIL，函数未定义。

- [ ] **Step 2: 实现 collect_time_parts**

`config.rs` 追加：

```rust
pub fn collect_time_parts(t: &str) -> Result<(u32, u32), ConfigError> {
    validate_collect_time(t)?;
    Ok((t[..2].parse().unwrap(), t[3..].parse().unwrap()))
}
```

Run: `cd backend && cargo test -p ght-core`
Expected: PASS。

- [ ] **Step 3: 实现 main.rs**

```rust
mod collect;
mod graphql;
mod search;
mod store;
mod trending;

use clap::Parser;
use collect::Collector;
use ght_core::config::{collect_time_parts, Settings};
use ght_core::db;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "ght-collector", about = "GitHub leaderboard collector")]
struct Cli {
    /// Run a single collection and exit (for CronJob / manual runs)
    #[arg(long)]
    once: bool,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("gh-trending-collector/0.1")
        .build()
        .expect("failed to build http client")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let settings = Settings::from_env()?;
    let pool = db::pg_pool(&settings.database_url).await?;
    db::migrate(&pool).await?;

    let collector = Arc::new(Collector {
        pool,
        http: http_client(),
        settings: settings.clone(),
        github_base: "https://github.com".to_string(),
        api_base: "https://api.github.com".to_string(),
    });

    if cli.once {
        let report = collector.collect_once().await;
        tracing::info!(ok = report.ok, failed = report.failed, "collection finished");
        anyhow::ensure!(report.ok > 0, "all collection tasks failed");
        return Ok(());
    }

    let (hour, minute) = collect_time_parts(&settings.collect_time)?;
    let cron = format!("0 {minute} {hour} * * *");
    let scheduler = JobScheduler::new().await?;
    let ctx = collector.clone();
    scheduler
        .add(Job::new_async(cron.as_str(), move |_uuid, _lock| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let report = ctx.collect_once().await;
                tracing::info!(ok = report.ok, failed = report.failed, "scheduled collection finished");
            })
        })?)
        .await?;
    scheduler.start().await?;
    tracing::info!(collect_time = %settings.collect_time, "collector daemon started");
    tokio::signal::ctrl_c().await?;
    tracing::info!("collector shutting down");
    Ok(())
}
```

- [ ] **Step 4: 编译验证**

Run: `cd backend && cargo build -p ght-collector`
Expected: 编译成功。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(collector): cli entrypoint with once mode and daily cron scheduling"
```

---

### Task 8: core 认证数据层（users + invite_codes + refresh_tokens）

**Files:**
- Create: `backend/crates/core/src/users.rs`、`backend/crates/core/src/refresh.rs`
- Modify: `backend/crates/core/src/lib.rs`（加模块）、`backend/crates/core/Cargo.toml`（加 `rand`）
- Test: 两文件各自 `#[cfg(test)]`（需测试库）

**Interfaces:**
- Produces（api 与 admin 共用）：
  - `users::{UserRow, InviteRow, InviteError, create_user, find_user_by_username, find_username_by_id, register_with_invite, create_invite, generate_invite_code, list_invites, revoke_invite}`
  - `refresh::{RefreshError, create_refresh_token, rotate_refresh_token, delete_refresh_token, delete_all_for_user}`
  - `UserRow { id: i64, username: String, password_hash: String }`
  - `InviteRow { code: String, max_uses: i32, used_count: i32, revoked: bool }`
  - `create_refresh_token(pool, user_id) -> Result<String, sqlx::Error>`（返回明文 token）
  - `rotate_refresh_token(pool, presented: &str) -> Result<i64, RefreshError>`（返回 user_id）

- [ ] **Step 1: 写 users.rs 失败测试**

```rust
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone)]
pub struct InviteRow {
    pub code: String,
    pub max_uses: i32,
    pub used_count: i32,
    pub revoked: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum InviteError {
    #[error("invite code not found")]
    NotFound,
    #[error("invite code revoked")]
    Revoked,
    #[error("invite code exhausted")]
    Exhausted,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UserError {
    #[error("username already taken")]
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn register_with_invite_consumes_once() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 1).await.unwrap();
        let uid = register_with_invite(&pool, "alice", "hash1", &code).await.unwrap();
        assert!(uid > 0);
        // 一次性邀请码第二次使用失败
        assert_eq!(
            register_with_invite(&pool, "bob", "hash2", &code).await.unwrap_err(),
            RegisterError::Invite(InviteError::Exhausted)
        );
        let invites = list_invites(&pool).await.unwrap();
        assert_eq!(invites[0].used_count, 1);
    }

    #[tokio::test]
    async fn register_rejects_revoked_and_unknown_codes() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 5).await.unwrap();
        assert!(revoke_invite(&pool, &code).await.unwrap());
        assert_eq!(
            register_with_invite(&pool, "carol", "h", &code).await.unwrap_err(),
            RegisterError::Invite(InviteError::Revoked)
        );
        assert_eq!(
            register_with_invite(&pool, "dave", "h", "NOPE").await.unwrap_err(),
            RegisterError::Invite(InviteError::NotFound)
        );
    }

    #[tokio::test]
    async fn duplicate_username_rejected_and_invite_not_consumed() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 1).await.unwrap();
        register_with_invite(&pool, "alice", "h1", &code).await.unwrap();
        let code2 = create_invite(&pool, 1).await.unwrap();
        assert_eq!(
            register_with_invite(&pool, "alice", "h2", &code2).await.unwrap_err(),
            RegisterError::DuplicateUsername
        );
        // code2 未被消耗
        let invites = list_invites(&pool).await.unwrap();
        let c2 = invites.iter().find(|i| i.code == code2).unwrap();
        assert_eq!(c2.used_count, 0);
    }

    #[tokio::test]
    async fn multi_use_invite_serves_n_users() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 2).await.unwrap();
        register_with_invite(&pool, "u1", "h", &code).await.unwrap();
        register_with_invite(&pool, "u2", "h", &code).await.unwrap();
        assert_eq!(
            register_with_invite(&pool, "u3", "h", &code).await.unwrap_err(),
            RegisterError::Invite(InviteError::Exhausted)
        );
    }

    #[tokio::test]
    fn invite_code_shape() {
        let code = generate_invite_code();
        assert_eq!(code.len(), 16);
        assert!(code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
```

注意测试中引用了 `RegisterError`——它是本任务要定义的核心类型，下一步实现。

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test -p ght-core users`
Expected: 编译失败。

- [ ] **Step 3: 实现 users.rs**

在测试模块之前：

```rust
use rand::RngCore;
use sqlx::PgPool;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RegisterError {
    #[error("invite: {0}")]
    Invite(#[from] InviteError),
    #[error("username already taken")]
    DuplicateUsername,
    #[error("db: {0}")]
    Db(String),
}

impl From<sqlx::Error> for RegisterError {
    fn from(e: sqlx::Error) -> Self {
        RegisterError::Db(e.to_string())
    }
}

pub async fn create_user(pool: &PgPool, username: &str, password_hash: &str, invite_id: Option<i64>) -> Result<i64, UserError> {
    let res = sqlx::query_scalar!(
        r#"INSERT INTO users (username, password_hash, created_by_invite)
           VALUES ($1, $2, $3)
           ON CONFLICT (username) DO NOTHING
           RETURNING id"#,
        username,
        password_hash,
        invite_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| UserError::Duplicate)?;
    res.ok_or(UserError::Duplicate)
}

pub async fn find_user_by_username(pool: &PgPool, username: &str) -> Result<Option<UserRow>, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT id, username, password_hash FROM users WHERE username = $1",
        username
    )
    .fetch_optional(pool)
    .await?;
    Ok(rec.map(|r| UserRow { id: r.id, username: r.username, password_hash: r.password_hash }))
}

pub async fn find_username_by_id(pool: &PgPool, user_id: i64) -> Result<Option<String>, sqlx::Error> {
    let rec = sqlx::query!("SELECT username FROM users WHERE id = $1", user_id)
        .fetch_optional(pool)
        .await?;
    Ok(rec.map(|r| r.username))
}

pub fn generate_invite_code() -> String {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn create_invite(pool: &PgPool, max_uses: i32) -> Result<String, sqlx::Error> {
    let code = generate_invite_code();
    sqlx::query!(
        "INSERT INTO invite_codes (code, max_uses) VALUES ($1, $2)",
        code,
        max_uses
    )
    .execute(pool)
    .await?;
    Ok(code)
}

/// 事务内完成：锁邀请码 → 校验 → 递增 used_count → 建用户。任何一步失败整体回滚。
pub async fn register_with_invite(pool: &PgPool, username: &str, password_hash: &str, code: &str) -> Result<i64, RegisterError> {
    let mut tx = pool.begin().await?;

    let invite = sqlx::query!(
        "SELECT id, max_uses, used_count, revoked FROM invite_codes WHERE code = $1 FOR UPDATE",
        code
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(InviteError::NotFound)?;

    if invite.revoked {
        return Err(RegisterError::Invite(InviteError::Revoked));
    }
    if invite.used_count >= invite.max_uses {
        return Err(RegisterError::Invite(InviteError::Exhausted));
    }

    let user_id = sqlx::query_scalar!(
        r#"INSERT INTO users (username, password_hash, created_by_invite)
           VALUES ($1, $2, $3)
           ON CONFLICT (username) DO NOTHING
           RETURNING id"#,
        username,
        password_hash,
        Some(invite.id)
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RegisterError::DuplicateUsername)?;

    sqlx::query!(
        "UPDATE invite_codes SET used_count = used_count + 1 WHERE id = $1",
        invite.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(user_id)
}

pub async fn list_invites(pool: &PgPool) -> Result<Vec<InviteRow>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT code, max_uses, used_count, revoked FROM invite_codes ORDER BY id DESC"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| InviteRow { code: r.code, max_uses: r.max_uses, used_count: r.used_count, revoked: r.revoked })
        .collect())
}

pub async fn revoke_invite(pool: &PgPool, code: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query!("UPDATE invite_codes SET revoked = true WHERE code = $1", code)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
```

并把文件开头的结构体/错误定义（UserRow、InviteRow、InviteError、UserError）放在 use 之后、实现之前（测试中已引用）。`lib.rs` 加 `pub mod users;`，`core/Cargo.toml` `[dependencies]` 加 `rand = { workspace = true }`。

- [ ] **Step 4: 运行 users 测试确认通过**

Run: `cd backend && cargo test -p ght-core users`
Expected: 全部 PASS。

- [ ] **Step 5: 写 refresh.rs 失败测试**

```rust
use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RefreshError {
    #[error("token not found")]
    Invalid,
    #[error("token expired")]
    Expired,
    #[error("token reuse detected; all sessions revoked")]
    Stolen,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_pool_with_user() -> (PgPool, i64) {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        let uid: i64 = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('u', 'h') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
        (pool, uid)
    }

    #[tokio::test]
    async fn rotate_returns_user_and_invalidates_old_token() {
        let (pool, uid) = test_pool_with_user().await;
        let token = create_refresh_token(&pool, uid).await.unwrap();
        let got = rotate_refresh_token(&pool, &token).await.unwrap();
        assert_eq!(got, uid);
        // 旧 token 再次使用 = 被盗，全部 token 被删
        assert_eq!(rotate_refresh_token(&pool, &token).await.unwrap_err(), RefreshError::Stolen);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn unknown_token_is_invalid() {
        let (pool, _uid) = test_pool_with_user().await;
        assert_eq!(rotate_refresh_token(&pool, "bogus").await.unwrap_err(), RefreshError::Invalid);
    }

    #[tokio::test]
    async fn expired_token_rejected() {
        let (pool, uid) = test_pool_with_user().await;
        let token = create_refresh_token(&pool, uid).await.unwrap();
        sqlx::query("UPDATE refresh_tokens SET expires_at = now() - interval '1 day'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(rotate_refresh_token(&pool, &token).await.unwrap_err(), RefreshError::Expired);
    }

    #[tokio::test]
    async fn delete_token_and_delete_all_for_user() {
        let (pool, uid) = test_pool_with_user().await;
        let t1 = create_refresh_token(&pool, uid).await.unwrap();
        let _t2 = create_refresh_token(&pool, uid).await.unwrap();
        delete_refresh_token(&pool, &t1).await.unwrap();
        delete_all_for_user(&pool, uid).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
```

- [ ] **Step 6: 实现 refresh.rs**

token 生成/哈希放在 core（api 也要用），refresh.rs 顶部：

```rust
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const REFRESH_TTL_SECS: i64 = 30 * 24 * 3600;

pub fn new_refresh_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn create_refresh_token(pool: &PgPool, user_id: i64) -> Result<String, sqlx::Error> {
    let token = new_refresh_token();
    let hash = hash_token(&token);
    let expires = Utc::now() + chrono::Duration::seconds(REFRESH_TTL_SECS);
    sqlx::query!(
        "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
        user_id,
        hash,
        expires
    )
    .execute(pool)
    .await?;
    Ok(token)
}

pub async fn rotate_refresh_token(pool: &PgPool, presented: &str) -> Result<i64, RefreshError> {
    let hash = hash_token(presented);
    let mut tx = pool.begin().await.map_err(|_| RefreshError::Invalid)?;

    let row = sqlx::query!(
        "SELECT id, user_id, expires_at, used_at FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
        hash
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefreshError::Invalid)?
    .ok_or(RefreshError::Invalid)?;

    if row.used_at.is_some() {
        let _ = sqlx::query!("DELETE FROM refresh_tokens WHERE user_id = $1", row.user_id)
            .execute(&mut *tx)
            .await;
        let _ = tx.commit().await;
        return Err(RefreshError::Stolen);
    }
    if row.expires_at < Utc::now() {
        let _ = sqlx::query!("DELETE FROM refresh_tokens WHERE id = $1", row.id)
            .execute(&mut *tx)
            .await;
        let _ = tx.commit().await;
        return Err(RefreshError::Expired);
    }

    sqlx::query!("UPDATE refresh_tokens SET used_at = now() WHERE id = $1", row.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Invalid)?;

    tx.commit().await.map_err(|_| RefreshError::Invalid)?;
    Ok(row.user_id)
}

pub async fn delete_refresh_token(pool: &PgPool, presented: &str) -> Result<(), sqlx::Error> {
    let hash = hash_token(presented);
    sqlx::query!("DELETE FROM refresh_tokens WHERE token_hash = $1", hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_all_for_user(pool: &PgPool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM refresh_tokens WHERE user_id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

`lib.rs` 加 `pub mod refresh;`；`core/Cargo.toml` 加 `sha2 = { workspace = true }`、`chrono = { workspace = true }`（已有则跳过）。

- [ ] **Step 7: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-core`
Expected: 全部 PASS。

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(core): user/invite registration and rotating refresh token store"
```

---

### Task 9: api 认证纯逻辑（JWT + 密码 + cookie）

**Files:**
- Create: `backend/crates/api/src/auth/mod.rs`、`tokens.rs`、`passwords.rs`、`cookies.rs`
- Modify: `backend/crates/api/src/lib.rs`（`pub mod auth;`）、`backend/crates/api/Cargo.toml`（加 `ght-core = { path = "../core" }`、`jsonwebtoken、rand、sha2、bcrypt、chrono`）
- Test: 三文件各自 `#[cfg(test)]`（纯逻辑，不需要 DB）

**Interfaces:**
- Produces:
  - `tokens::{Claims, ACCESS_TTL_SECS, issue_access, verify_access}`；`Claims { sub: i64, username: String, iat: i64, exp: i64 }`
  - `passwords::{hash_password, verify_password}`
  - `cookies::{access_cookie, refresh_cookie, clear_cookies, read_cookie}`
- Consumes: Task 8 `ght_core::refresh::{new_refresh_token, hash_token}`（api 不重复实现 token 生成）

- [ ] **Step 1: 写 tokens.rs 失败测试**

`auth/mod.rs`：

```rust
pub mod cookies;
pub mod passwords;
pub mod tokens;
```

`tokens.rs` 测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_claims() {
        let token = issue_access("secret", 7, "alice").unwrap();
        let claims = verify_access("secret", &token).unwrap();
        assert_eq!(claims.sub, 7);
        assert_eq!(claims.username, "alice");
        assert!(claims.exp - claims.iat == ACCESS_TTL_SECS);
    }

    #[test]
    fn wrong_secret_rejected() {
        let token = issue_access("secret", 7, "alice").unwrap();
        assert!(verify_access("other", &token).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let claims = Claims { sub: 1, username: "x".into(), iat: 0, exp: 1 };
        let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(b"secret")).unwrap();
        assert!(verify_access("secret", &token).is_err());
    }
}
```

Run: `cd backend && cargo test -p ght-api`
Expected: 编译失败。

- [ ] **Step 2: 实现 tokens.rs**

```rust
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

pub const ACCESS_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub iat: i64,
    pub exp: i64,
}

pub fn issue_access(secret: &str, user_id: i64, username: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + ACCESS_TTL_SECS,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
}

pub fn verify_access(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default())?;
    Ok(data.claims)
}
```

- [ ] **Step 3: 写 passwords.rs（测试 + 实现）**

```rust
pub fn hash_password(plain: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(plain, bcrypt::DEFAULT_COST)
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    bcrypt::verify(plain, hash).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("s3cret-pw").unwrap();
        assert!(verify_password("s3cret-pw", &hash));
        assert!(!verify_password("wrong", &hash));
    }
}
```

- [ ] **Step 4: 写 cookies.rs（测试 + 实现）**

```rust
pub fn access_cookie(jwt: &str, secure: bool) -> String {
    format!(
        "access_token={jwt}; HttpOnly; SameSite=Lax; Path=/; Max-Age=900{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn refresh_cookie(token: &str, secure: bool) -> String {
    format!(
        "refresh_token={token}; HttpOnly; SameSite=Lax; Path=/api/auth; Max-Age=2592000{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookies() -> Vec<String> {
    vec![
        "access_token=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string(),
        "refresh_token=; HttpOnly; SameSite=Lax; Path=/api/auth; Max-Age=0".to_string(),
    ]
}

pub fn read_cookie(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k.trim() == name {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_access_cookie() {
        let c = access_cookie("jwt.value", false);
        assert!(c.starts_with("access_token=jwt.value; HttpOnly; SameSite=Lax; Path=/; Max-Age=900"));
        assert!(!c.contains("Secure"));
        assert!(access_cookie("j", true).ends_with("; Secure"));
    }

    #[test]
    fn refresh_cookie_scoped_to_auth_path() {
        assert!(refresh_cookie("t", false).contains("Path=/api/auth"));
    }

    #[test]
    fn reads_named_cookie() {
        let header = "a=1; access_token=xyz; b=2";
        assert_eq!(read_cookie(header, "access_token").as_deref(), Some("xyz"));
        assert_eq!(read_cookie(header, "missing"), None);
    }
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-api`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(api): stateless jwt tokens, bcrypt passwords, cookie helpers"
```

---

### Task 10: api 状态 + RequireAuth extractor + auth 路由

**Files:**
- Create: `backend/crates/api/src/state.rs`、`backend/crates/api/src/auth/extract.rs`、`backend/crates/api/src/auth/routes.rs`
- Modify: `backend/crates/api/src/auth/mod.rs`、`backend/crates/api/src/lib.rs`
- Test: `auth/routes.rs` 内 `#[cfg(test)]`（axum Router 直驱 + 测试库）

**Interfaces:**
- Consumes: Task 1 `Settings`、Task 8 `ght_core::{users, refresh}`、Task 9 `auth::{tokens, passwords, cookies}`
- Produces:
  - `state::AppState { pool: PgPool, settings: Settings }`（`#[derive(Clone)]`）
  - `auth::extract::RequireAuth(pub tokens::Claims)`：axum `FromRequestParts<AppState>`，失败返回 401 JSON
  - `auth::routes::router(state) -> Router<AppState>`：挂载 `/register /login /refresh /me /logout`
  - 请求/响应 DTO：`RegisterReq { username, password, invite_code }`、`LoginReq { username, password }`、`MeResp { user_id, username }`、`AuthResp { username }`

- [ ] **Step 1: state.rs 与 lib.rs**

`state.rs`：

```rust
use ght_core::config::Settings;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub settings: Settings,
}
```

`lib.rs`：

```rust
pub mod auth;
pub mod state;
```

- [ ] **Step 2: extract.rs（RequireAuth）**

```rust
use super::cookies;
use super::tokens::{self, Claims};
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub struct RequireAuth(pub Claims);

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response()
    }
}

pub struct AuthError;

impl FromRequestParts<AppState> for RequireAuth {
    type Rejection = AuthError;

    fn from_request_parts<'a>(
        parts: &'a mut Parts,
        state: &'a AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let header = parts
                .headers
                .get(axum::http::header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .ok_or(AuthError)?;
            let token = cookies::read_cookie(header, "access_token").ok_or(AuthError)?;
            let claims = tokens::verify_access(&state.settings.jwt_secret, &token).map_err(|_| AuthError)?;
            Ok(RequireAuth(claims))
        }
    }
}
```

- [ ] **Step 3: 写 auth/routes.rs 失败测试**

`routes.rs` 测试模块（先写，路由实现下一步补）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ght_core::{db, users};
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url),
            "JWT_SECRET" => Some("test-secret".into()),
            _ => None,
        })
        .unwrap();
        AppState { pool, settings }
    }

    fn json_request(method: &str, uri: &str, body: &str, cookie: Option<&str>) -> Request<Body> {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        req.body(Body::from(body.to_string())).unwrap()
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn set_cookie(resp: &axum::response::Response, name: &str) -> Option<String> {
        resp.headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with(&format!("{name}=")))
            .map(|c| c.split(';').next().unwrap().to_string())
    }

    #[tokio::test]
    async fn register_login_me_flow() {
        let state = test_state().await;
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        let app = router(state.clone());

        // register
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/register",
                &format!(r#"{{"username":"alice","password":"password123","invite_code":"{invite}"}}"#),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let access = set_cookie(&resp, "access_token").unwrap();
        assert!(set_cookie(&resp, "refresh_token").is_some());

        // me with access cookie
        let resp = app
            .clone()
            .oneshot(json_request("GET", "/me", "", Some(&access)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("alice"));

        // me without cookie -> 401
        let resp = app.clone().oneshot(json_request("GET", "/me", "", None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // login with correct password
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/login", r#"{"username":"alice","password":"password123"}"#, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // login with wrong password -> 401
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/login", r#"{"username":"alice","password":"wrong"}"#, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn register_with_bad_invite_fails() {
        let state = test_state().await;
        let app = router(state.clone());
        let resp = app
            .oneshot(json_request(
                "POST",
                "/register",
                r#"{"username":"bob","password":"password123","invite_code":"NOPE"}"#,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn refresh_rotates_and_reuse_is_rejected() {
        let state = test_state().await;
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        let app = router(state.clone());

        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/register",
                &format!(r#"{{"username":"carol","password":"password123","invite_code":"{invite}"}}"#),
                None,
            ))
            .await
            .unwrap();
        let refresh = set_cookie(&resp, "refresh_token").unwrap();

        // refresh succeeds and issues new tokens
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/refresh", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let new_refresh = set_cookie(&resp, "refresh_token").unwrap();
        assert_ne!(new_refresh, refresh);

        // replaying the OLD refresh cookie -> 401 (theft detection)
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/refresh", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn logout_clears_cookies_and_invalidates_refresh() {
        let state = test_state().await;
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        let app = router(state.clone());
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/register",
                &format!(r#"{{"username":"dave","password":"password123","invite_code":"{invite}"}}"#),
                None,
            ))
            .await
            .unwrap();
        let refresh = set_cookie(&resp, "refresh_token").unwrap();

        let resp = app
            .clone()
            .oneshot(json_request("POST", "/logout", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // refresh after logout -> 401
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/refresh", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
```

- [ ] **Step 4: 运行确认失败**

Run: `cd backend && cargo test -p ght-api`
Expected: 编译失败，`router` 未定义。

- [ ] **Step 5: 实现 auth/routes.rs**

测试模块之前：

```rust
use super::cookies;
use super::extract::RequireAuth;
use super::passwords;
use super::tokens;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use ght_core::refresh as refresh_store;
use ght_core::users;
use serde::{Deserialize, Serialize};

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .with_state(state)
}

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub password: String,
    pub invite_code: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResp {
    pub username: String,
}

#[derive(Serialize)]
pub struct MeResp {
    pub user_id: i64,
    pub username: String,
}

fn unauthorized(msg: &str) -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": msg })),
    )
}

fn issue_cookies(state: &AppState, user_id: i64, username: &str) -> Result<Vec<String>, StatusCode> {
    let jwt = tokens::issue_access(&state.settings.jwt_secret, user_id, username).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(vec![cookies::access_cookie(&jwt, state.settings.cookie_secure)])
}

async fn register(State(state): State<AppState>, Json(req): Json<RegisterReq>) -> impl IntoResponse {
    if req.username.len() > 64 || req.password.len() > 256 {
        return unauthorized("invalid input").into_response();
    }
    let hash = match passwords::hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };
    match users::register_with_invite(&state.pool, &req.username, &hash, &req.invite_code).await {
        Ok(user_id) => {
            let refresh = match refresh_store::create_refresh_token(&state.pool, user_id).await {
                Ok(t) => t,
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            };
            let mut cookies = match issue_cookies(&state, user_id, &req.username) {
                Ok(c) => c,
                Err(sc) => return (sc).into_response(),
            };
            cookies.push(cookies::refresh_cookie(&refresh, state.settings.cookie_secure));
            let mut resp = (StatusCode::OK, Json(AuthResp { username: req.username })).into_response();
            for c in cookies {
                resp.headers_mut().append("set-cookie", c.parse().unwrap());
            }
            resp
        }
        Err(e) => unauthorized(&e.to_string()).into_response(),
    }
}

async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> impl IntoResponse {
    let user = match users::find_user_by_username(&state.pool, &req.username).await {
        Ok(Some(u)) => u,
        _ => return unauthorized("invalid credentials").into_response(),
    };
    if !passwords::verify_password(&req.password, &user.password_hash) {
        return unauthorized("invalid credentials").into_response();
    }
    let refresh = match refresh_store::create_refresh_token(&state.pool, user.id).await {
        Ok(t) => t,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };
    let mut cookies = match issue_cookies(&state, user.id, &user.username) {
        Ok(c) => c,
        Err(sc) => return (sc).into_response(),
    };
    cookies.push(cookies::refresh_cookie(&refresh, state.settings.cookie_secure));
    let mut resp = (StatusCode::OK, Json(AuthResp { username: user.username })).into_response();
    for c in cookies {
        resp.headers_mut().append("set-cookie", c.parse().unwrap());
    }
    resp
}

fn read_refresh_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    cookies::read_cookie(header, "refresh_token")
}

async fn refresh(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    let presented = match read_refresh_cookie(&headers) {
        Some(t) => t,
        None => return unauthorized("missing refresh token").into_response(),
    };
    match refresh_store::rotate_refresh_token(&state.pool, &presented).await {
        Ok(user_id) => {
            let username = match users::find_username_by_id(&state.pool, user_id).await {
                Ok(Some(u)) => u,
                _ => return unauthorized("unknown user").into_response(),
            };
            let new_refresh = match refresh_store::create_refresh_token(&state.pool, user_id).await {
                Ok(t) => t,
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            };
            let mut cookies = match issue_cookies(&state, user_id, &username) {
                Ok(c) => c,
                Err(sc) => return (sc).into_response(),
            };
            cookies.push(cookies::refresh_cookie(&new_refresh, state.settings.cookie_secure));
            let mut resp = (StatusCode::OK, Json(AuthResp { username })).into_response();
            for c in cookies {
                resp.headers_mut().append("set-cookie", c.parse().unwrap());
            }
            resp
        }
        Err(_) => {
            let mut resp = unauthorized("invalid refresh token").into_response();
            for c in cookies::clear_cookies() {
                resp.headers_mut().append("set-cookie", c.parse().unwrap());
            }
            resp
        }
    }
}

async fn logout(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if let Some(t) = read_refresh_cookie(&headers) {
        let _ = refresh_store::delete_refresh_token(&state.pool, &t).await;
    }
    let mut resp = (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response();
    for c in cookies::clear_cookies() {
        resp.headers_mut().append("set-cookie", c.parse().unwrap());
    }
    resp
}

async fn me(RequireAuth(claims): RequireAuth) -> impl IntoResponse {
    Json(MeResp {
        user_id: claims.sub,
        username: claims.username,
    })
}
```

职责约定（Task 8 已按此实现）：`rotate_refresh_token` 只负责校验 + 作废（置 `used_at`）旧 token，返回 user_id，**不插入新行**；新 refresh token 由本 handler 调 `create_refresh_token` 签发。二者各管一段，避免双插。

- [ ] **Step 6: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-api && cargo test -p ght-core refresh`
Expected: 全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(api): state/extract/auth routes with rotating refresh tokens"
```

---

### Task 11: 榜单路由 + 静态文件 + api main

**Files:**
- Create: `backend/crates/api/src/routes_leaderboard.rs`
- Modify: `backend/crates/api/src/lib.rs`（加 `build_router`）
- Modify: `backend/crates/api/src/main.rs`
- Test: `routes_leaderboard.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 2 `ght_core::store::{top_by_stars, top_by_forks, top_by_watchers, trending, latest_snapshot_date, languages_with_counts}`、`models::Board`、Task 10 `state::AppState`、`auth::extract::RequireAuth`、`auth::routes::router`
- Produces: `lib::build_router(state: AppState) -> Router`（含 `/api/auth/*`、`/api/leaderboard/*`、`/api/languages`、`/api/meta`、`/api/health` 与静态 fallback）
- 端点（spec §6）：`GET /api/leaderboard/top?metric=stars|forks|watchers&language=`、`GET /api/leaderboard/trending?language=`、`GET /api/languages`、`GET /api/meta`、`GET /api/health`

- [ ] **Step 1: 写 routes_leaderboard.rs 失败测试**

```rust
#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
    use ght_core::models::{Board, RepoInput, SnapshotInput};
    use ght_core::store as store;
    use ght_core::{db, users};
    use tower::ServiceExt;

    use crate::auth::tokens;
    use crate::state::AppState;

    async fn test_state() -> AppState {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url),
            "JWT_SECRET" => Some("test-secret".into()),
            _ => None,
        })
        .unwrap();
        AppState { pool, settings }
    }

    fn repo(full_name: &str, lang: Option<&str>) -> RepoInput {
        let (owner, name) = full_name.split_once('/').unwrap();
        RepoInput {
            full_name: full_name.into(),
            owner: owner.into(),
            name: name.into(),
            html_url: format!("https://github.com/{full_name}"),
            language: lang.map(String::from),
            description: None,
        }
    }

    async fn seed(state: &AppState) {
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (name, lang, stars, forks, watchers) in [
            ("a/py1", Some("Python"), 300, 30, 3),
            ("a/py2", Some("Python"), 100, 50, 9),
            ("a/rs1", Some("Rust"), 200, 10, 1),
        ] {
            let id = store::upsert_repo(&state.pool, &repo(name, lang), date).await.unwrap();
            store::upsert_snapshot(&state.pool, id, date, Board::TopStars, &SnapshotInput { stars, forks, watchers: Some(watchers), stars_today: None }).await.unwrap();
            store::upsert_snapshot(&state.pool, id, date, Board::TopForks, &SnapshotInput { stars, forks, watchers: Some(watchers), stars_today: None }).await.unwrap();
            store::upsert_snapshot(&state.pool, id, date, Board::TopWatchers, &SnapshotInput { stars, forks, watchers: Some(watchers), stars_today: None }).await.unwrap();
            store::upsert_snapshot(&state.pool, id, date, Board::TrendingDaily, &SnapshotInput { stars: stars / 10, forks, watchers: None, stars_today: Some(stars / 10) }).await.unwrap();
        }
    }

    fn auth_cookie(state: &AppState) -> String {
        let jwt = tokens::issue_access(&state.settings.jwt_secret, 1, "tester").unwrap();
        format!("access_token={jwt}")
    }

    async fn get(state: AppState, uri: &str, cookie: Option<String>) -> (StatusCode, String) {
        let mut builder = Request::builder().method("GET").uri(uri);
        if let Some(c) = cookie {
            builder = builder.header("cookie", c);
        }
        let resp = crate::build_router(state)
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn top_by_stars_and_language_filter() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);

        let (status, body) = get(state.clone(), "/api/leaderboard/top?metric=stars", Some(cookie.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"full_name\":\"a/py1\""));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"][0]["rank"], 1);
        assert_eq!(v["items"][0]["full_name"], "a/py1");

        let (_, body) = get(state.clone(), "/api/leaderboard/top?metric=stars&language=Rust", Some(cookie.clone())).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["full_name"], "a/rs1");
        assert_eq!(v["items"][0]["rank"], 1);
    }

    #[tokio::test]
    async fn top_requires_auth() {
        let state = test_state().await;
        seed(&state).await;
        let (status, _) = get(state.clone(), "/api/leaderboard/top?metric=stars", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn trending_returns_stars_today() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);
        let (status, body) = get(state.clone(), "/api/leaderboard/trending", Some(cookie)).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"][0]["full_name"], "a/py1");
        assert_eq!(v["items"][0]["stars_today"], 30);
    }

    #[tokio::test]
    async fn languages_and_meta_and_health() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);

        let (_, body) = get(state.clone(), "/api/languages", Some(cookie.clone())).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let langs: Vec<&str> = v.as_array().unwrap().iter().map(|x| x["language"].as_str().unwrap()).collect();
        assert!(langs.contains(&"Python") && langs.contains(&"Rust"));

        let (_, body) = get(state.clone(), "/api/meta", Some(cookie.clone())).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["date"], "2026-08-06");
        assert_eq!(v["boards"]["top_stars"], 3);
        assert_eq!(v["boards"]["trending_daily"], 3);

        let (status, _) = get(state.clone(), "/api/health", None).await;
        assert_eq!(status, StatusCode::OK);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test -p ght-api leaderboard`
Expected: 编译失败，`build_router`/handlers 未定义。

- [ ] **Step 3: 实现 routes_leaderboard.rs**

测试模块之前：

```rust
use crate::auth::extract::RequireAuth;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use ght_core::models::Board;
use ght_core::store as store;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct TopParams {
    pub metric: String,
    pub language: Option<String>,
}

#[derive(Deserialize)]
pub struct TrendingParams {
    pub language: Option<String>,
}

#[derive(Serialize)]
pub struct LeaderboardItem {
    pub rank: i64,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}

#[derive(Serialize)]
pub struct LeaderboardResp {
    pub date: String,
    pub board: String,
    pub language: Option<String>,
    pub items: Vec<LeaderboardItem>,
}

fn to_items(rows: Vec<ght_core::models::LeaderboardRow>) -> Vec<LeaderboardItem> {
    rows.into_iter()
        .map(|r| LeaderboardItem {
            rank: r.rank,
            full_name: r.full_name,
            html_url: r.html_url,
            description: r.description,
            language: r.language,
            stars: r.stars,
            forks: r.forks,
            watchers: r.watchers,
            stars_today: r.stars_today,
        })
        .collect()
}

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/leaderboard/top", axum::routing::get(top))
        .route("/api/leaderboard/trending", axum::routing::get(trending))
        .route("/api/languages", axum::routing::get(languages))
        .route("/api/meta", axum::routing::get(meta))
        .route("/api/health", axum::routing::get(health))
        .with_state(state)
}

async fn top(
    State(state): State<AppState>,
    _auth: RequireAuth,
    Query(params): Query<TopParams>,
) -> impl IntoResponse {
    let board = match params.metric.as_str() {
        "stars" => Board::TopStars,
        "forks" => Board::TopForks,
        "watchers" => Board::TopWatchers,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"metric must be stars|forks|watchers"}))).into_response(),
    };
    let date = match store::latest_snapshot_date(&state.pool, board).await {
        Ok(Some(d)) => d,
        _ => {
            return Json(LeaderboardResp {
                date: String::new(),
                board: board.as_str().to_string(),
                language: params.language.clone(),
                items: vec![],
            })
            .into_response()
        }
    };
    let lang = params.language.as_deref();
    let rows = match board {
        Board::TopStars => store::top_by_stars(&state.pool, date, lang, 100).await,
        Board::TopForks => store::top_by_forks(&state.pool, date, lang, 100).await,
        Board::TopWatchers => store::top_by_watchers(&state.pool, date, lang, 100).await,
        _ => unreachable!(),
    };
    match rows {
        Ok(rows) => Json(LeaderboardResp {
            date: date.format("%Y-%m-%d").to_string(),
            board: board.as_str().to_string(),
            language: params.language.clone(),
            items: to_items(rows),
        })
        .into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    }
}

async fn trending(
    State(state): State<AppState>,
    _auth: RequireAuth,
    Query(params): Query<TrendingParams>,
) -> impl IntoResponse {
    let board = Board::TrendingDaily;
    let date = match store::latest_snapshot_date(&state.pool, board).await {
        Ok(Some(d)) => d,
        _ => {
            return Json(LeaderboardResp {
                date: String::new(),
                board: board.as_str().to_string(),
                language: params.language.clone(),
                items: vec![],
            })
            .into_response()
        }
    };
    match store::trending(&state.pool, date, params.language.as_deref(), 100).await {
        Ok(rows) => Json(LeaderboardResp {
            date: date.format("%Y-%m-%d").to_string(),
            board: board.as_str().to_string(),
            language: params.language.clone(),
            items: to_items(rows),
        })
        .into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    }
}

async fn languages(State(state): State<AppState>, _auth: RequireAuth) -> impl IntoResponse {
    let date = match store::latest_snapshot_date(&state.pool, Board::TopStars).await {
        Ok(Some(d)) => d,
        _ => return Json(serde_json::json!([])).into_response(),
    };
    match store::languages_with_counts(&state.pool, date).await {
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(language, count)| serde_json::json!({ "language": language, "count": count }))
                .collect();
            Json(serde_json::json!(items)).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    }
}

async fn meta(State(state): State<AppState>, _auth: RequireAuth) -> impl IntoResponse {
    let date = store::latest_snapshot_date(&state.pool, Board::TopStars).await.ok().flatten();
    let mut counts = serde_json::Map::new();
    if let Some(d) = date {
        for board in [Board::TopStars, Board::TopForks, Board::TopWatchers, Board::TrendingDaily] {
            let n = store::board_count(&state.pool, d, board).await.unwrap_or(0);
            counts.insert(board.as_str().to_string(), serde_json::json!(n));
        }
    }
    Json(serde_json::json!({
        "date": date.map(|d| d.format("%Y-%m-%d").to_string()),
        "boards": counts,
    }))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}
```

- [ ] **Step 4: lib.rs 组装 build_router**

`lib.rs`：

```rust
pub mod auth;
pub mod routes_leaderboard;
pub mod state;

use axum::Router;
use state::AppState;

pub fn build_router(state: AppState) -> Router {
    let auth_router = auth::routes::router(state.clone());
    let board_router = routes_leaderboard::router(state.clone());
    Router::new()
        .nest("/api/auth", auth_router)
        .merge(board_router)
}
```

注意 `auth::mod.rs` 需 `pub mod routes; pub mod extract;`（extract/routes 已在 Task 10 建好，确保 mod.rs 声明齐全）。

- [ ] **Step 5: main.rs**

```rust
use ght_core::config::Settings;
use ght_core::db;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let settings = Settings::from_env()?;
    let pool = db::pg_pool(&settings.database_url).await?;
    db::migrate(&pool).await?;

    let state = ght_api::state::AppState { pool, settings };
    let app = ght_api::build_router(state)
        .layer(TraceLayer::new_for_http())
        .fallback_service(ServeDir::new("frontend/dist"));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    tracing::info!("api listening on :8000");
    axum::serve(listener, app).await?;
    Ok(())
}
```

`api/Cargo.toml` 确认含 `tower-http`（fs、trace feature）、`tokio`、`anyhow`、`tracing-subscriber`。

- [ ] **Step 6: 运行测试确认通过**

Run: `cd backend && cargo test -p ght-api`
Expected: 全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(api): leaderboard/languages/meta/health routes and static fallback"
```

---

### Task 12: admin CLI

**Files:**
- Modify: `backend/crates/admin/src/main.rs`（完整实现）
- Modify: `backend/crates/admin/Cargo.toml`（加 `ght-core = { path = "../core" }`、`clap、tokio、anyhow、tracing-subscriber`）
- Test: 手工验证（CLI 依赖真实库，单测覆盖在 core 已有）

**Interfaces:**
- Consumes: Task 1 `Settings`、Task 8 `ght_core::users::{create_user, create_invite, list_invites, revoke_invite}`、`ght_core::db`
- Produces: `ght-admin` 子命令：
  - `create-user --username <u> --password <p>`
  - `invite create [--uses N]`（默认 1）
  - `invite list`
  - `invite revoke <code>`

- [ ] **Step 1: 实现 main.rs**

```rust
use clap::{Parser, Subcommand};
use ght_core::config::Settings;
use ght_core::db;
use ght_core::users;

#[derive(Parser)]
#[command(name = "ght-admin", about = "GH Trending admin CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a user account (bootstrap; no invite code needed)
    CreateUser {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
    },
    /// Manage invite codes
    Invite {
        #[command(subcommand)]
        action: InviteAction,
    },
}

#[derive(Subcommand)]
enum InviteAction {
    /// Generate a new invite code
    Create {
        /// Number of times the code can be used
        #[arg(long, default_value_t = 1)]
        uses: i32,
    },
    /// List invite codes and usage
    List,
    /// Revoke an invite code
    Revoke { code: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let settings = Settings::from_env()?;
    let pool = db::pg_pool(&settings.database_url).await?;
    db::migrate(&pool).await?;

    match cli.command {
        Command::CreateUser { username, password } => {
            let hash = bcrypt_hash(&password)?;
            match users::create_user(&pool, &username, &hash, None).await {
                Ok(id) => println!("created user '{username}' (id={id})"),
                Err(users::UserError::Duplicate) => anyhow::bail!("username '{username}' already taken"),
            }
        }
        Command::Invite { action } => match action {
            InviteAction::Create { uses } => {
                anyhow::ensure!(uses > 0, "--uses must be positive");
                let code = users::create_invite(&pool, uses).await?;
                println!("{code}");
            }
            InviteAction::List => {
                let rows = users::list_invites(&pool).await?;
                if rows.is_empty() {
                    println!("(no invite codes)");
                }
                for r in rows {
                    let status = if r.revoked { "revoked" } else { "active" };
                    println!("{}\t{}/{} used\t{}", r.code, r.used_count, r.max_uses, status);
                }
            }
            InviteAction::Revoke { code } => {
                if users::revoke_invite(&pool, &code).await? {
                    println!("revoked {code}");
                } else {
                    anyhow::bail!("invite code '{code}' not found");
                }
            }
        },
    }
    Ok(())
}

fn bcrypt_hash(password: &str) -> anyhow::Result<String> {
    Ok(bcrypt::hash(password, bcrypt::DEFAULT_COST)?)
}
```

`admin/Cargo.toml` 依赖补 `bcrypt = { workspace = true }`。

- [ ] **Step 2: 手工端到端验证**

前置：`make db`，导出 `DATABASE_URL` 与 `JWT_SECRET`（可用 `.env.example` 复制为 `.env` 后 `export $(grep -v '^#' .env | xargs)`）。

Run:
```bash
cd backend
cargo run -p ght-admin -- create-user --username admin --password admin-pass-123
cargo run -p ght-admin -- invite create --uses 2
cargo run -p ght-admin -- invite list
```
Expected: 首条创建用户成功；第二条打印一个邀请码；第三条列表出现该码 `0/2 used active`。

Run: `cargo run -p ght-admin -- invite revoke <上一步的码> && cargo run -p ght-admin -- invite list`
Expected: 该码状态变为 `revoked`。

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat(admin): cli for bootstrap user and invite code management"
```

---

### Task 13: 前端脚手架 + API 客户端 + 类型

**Files:**
- Create: `frontend/`（Vite react-ts 模板）+ Tailwind 配置
- Create: `frontend/src/api.ts`、`frontend/src/types.ts`、`frontend/src/api.test.ts`
- Test: `vitest`

**Interfaces:**
- Produces: `api<T>(path, init?) -> Promise<T>`（401 自动 refresh 重放一次，再失败抛 `UnauthorizedError`）、`postAuth(path, body?) -> Promise<Response>`、`startRefreshTimer(onExpired) -> () => void`、`compact(n) -> string`
- Consumes: 后端 `/api/*` 契约（Task 10/11）

- [ ] **Step 1: 脚手架**

Run:
```bash
cd /Users/minwang/Projects/gh-trending
npm create vite@latest frontend -- --template react-ts
cd frontend && npm install
npm install -D tailwindcss@3 postcss autoprefixer vitest
npx tailwindcss init -p
```

删除模板的 `src/App.css`，`src/index.css` 替换为：

```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

`tailwind.config.js` content 设为 `["./index.html", "./src/**/*.{ts,tsx}"]`。

`vite.config.ts`：

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: { "/api": "http://localhost:8000" },
  },
});
```

`package.json` scripts 加 `"test": "vitest run"`。

- [ ] **Step 2: types.ts**

```ts
export interface LeaderboardItem {
  rank: number;
  full_name: string;
  html_url: string;
  description: string | null;
  language: string | null;
  stars: number;
  forks: number;
  watchers: number | null;
  stars_today: number | null;
}

export interface LeaderboardResponse {
  date: string;
  board: string;
  language: string | null;
  items: LeaderboardItem[];
}

export interface MeResponse {
  user_id: number;
  username: string;
}

export interface LanguageOption {
  language: string;
  count: number;
}
```

- [ ] **Step 3: 写失败测试 api.test.ts**

```ts
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { api, UnauthorizedError, compact } from "./api";

describe("compact", () => {
  it("formats numbers compactly", () => {
    expect(compact(190000)).toBe("190K");
    expect(compact(950)).toBe("950");
  });
});

describe("api 401 handling", () => {
  beforeEach(() => vi.restoreAllMocks());
  afterEach(() => vi.restoreAllMocks());

  it("retries once after successful refresh", async () => {
    const fetchMock = vi.fn()
      // 第一次业务请求 401
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      // refresh 成功
      .mockResolvedValueOnce(new Response(null, { status: 200 }))
      // 重放成功
      .mockResolvedValueOnce(new Response(JSON.stringify({ ok: 1 }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const data = await api<{ ok: number }>("/api/leaderboard/trending");
    expect(data.ok).toBe(1);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(fetchMock.mock.calls[1][0]).toBe("/api/auth/refresh");
  });

  it("throws UnauthorizedError when refresh fails", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(new Response(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/leaderboard/trending")).rejects.toBeInstanceOf(UnauthorizedError);
  });

  it("does not retry 401 on auth endpoints", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/auth/me")).rejects.toBeInstanceOf(UnauthorizedError);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
```

Run: `cd frontend && npm test`
Expected: FAIL（api.ts 不存在）。

- [ ] **Step 4: 实现 api.ts**

```ts
export class UnauthorizedError extends Error {
  constructor() {
    super("unauthorized");
  }
}

let refreshInFlight: Promise<boolean> | null = null;

export function tryRefresh(): Promise<boolean> {
  if (!refreshInFlight) {
    refreshInFlight = fetch("/api/auth/refresh", { method: "POST" })
      .then((r) => r.ok)
      .catch(() => false)
      .finally(() => {
        refreshInFlight = null;
      });
  }
  return refreshInFlight;
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const doFetch = () => fetch(path, init);
  let res = await doFetch();
  if (res.status === 401 && !path.startsWith("/api/auth/")) {
    const refreshed = await tryRefresh();
    if (refreshed) {
      res = await doFetch();
    }
    if (!res.ok) {
      throw new UnauthorizedError();
    }
    return res.json();
  }
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}`);
  }
  return res.json();
}

export async function postAuth(path: string, body?: unknown): Promise<Response> {
  return fetch(path, {
    method: "POST",
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
}

const REFRESH_INTERVAL_MS = 10 * 60 * 1000;

export function startRefreshTimer(onExpired: () => void): () => void {
  const tick = () => {
    if (document.visibilityState !== "visible") return;
    void tryRefresh().then((ok) => {
      if (!ok) onExpired();
    });
  };
  const timer = window.setInterval(tick, REFRESH_INTERVAL_MS);
  document.addEventListener("visibilitychange", tick);
  return () => {
    window.clearInterval(timer);
    document.removeEventListener("visibilitychange", tick);
  };
}

export function compact(n: number): string {
  return new Intl.NumberFormat("en", { notation: "compact" }).format(n);
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd frontend && npm test`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(frontend): vite scaffold, api client with refresh retry, types"
```

---

### Task 14: 登录/注册界面与登录态门

**Files:**
- Create: `frontend/src/components/AuthCard.tsx`
- Modify: `frontend/src/App.tsx`
- Test: 手工验证（配合后端）

**Interfaces:**
- Consumes: Task 13 `api`、`postAuth`、`startRefreshTimer`、`MeResponse`
- Produces: `AuthCard({ onLoggedIn: (username: string) => void })`；App 登录态三态（loading/anon/user）

- [ ] **Step 1: AuthCard.tsx**

```tsx
import { useState } from "react";
import { postAuth } from "../api";

type Mode = "login" | "register";

export default function AuthCard({ onLoggedIn }: { onLoggedIn: (username: string) => void }) {
  const [mode, setMode] = useState<Mode>("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [inviteCode, setInviteCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const path = mode === "login" ? "/api/auth/login" : "/api/auth/register";
    const body =
      mode === "login"
        ? { username, password }
        : { username, password, invite_code: inviteCode };
    try {
      const res = await postAuth(path, body);
      if (res.ok) {
        onLoggedIn(username);
      } else {
        const data = await res.json().catch(() => ({}));
        setError(data.error ?? `HTTP ${res.status}`);
      }
    } catch {
      setError("network error");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-neutral-950 text-neutral-100">
      <form onSubmit={submit} className="w-80 space-y-3 rounded-lg border border-neutral-800 p-6">
        <h1 className="text-lg font-semibold">GH Trending</h1>
        <input
          className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
          placeholder="username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
        <input
          className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
          placeholder="password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
        {mode === "register" && (
          <input
            className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
            placeholder="invite code"
            value={inviteCode}
            onChange={(e) => setInviteCode(e.target.value)}
            required
          />
        )}
        {error && <p className="text-sm text-red-400">{error}</p>}
        <button
          className="w-full rounded bg-emerald-600 py-2 text-sm font-medium hover:bg-emerald-500 disabled:opacity-50"
          disabled={busy}
        >
          {mode === "login" ? "Sign in" : "Create account"}
        </button>
        <button
          type="button"
          className="w-full text-sm text-neutral-400 hover:text-neutral-200"
          onClick={() => setMode(mode === "login" ? "register" : "login")}
        >
          {mode === "login" ? "No account? Register with invite code" : "Have an account? Sign in"}
        </button>
      </form>
    </div>
  );
}
```

- [ ] **Step 2: App.tsx 登录态门**

```tsx
import { useEffect, useState } from "react";
import AuthCard from "./components/AuthCard";
import Leaderboard from "./components/Leaderboard";
import { api, startRefreshTimer } from "./api";
import type { MeResponse } from "./types";

type AuthState =
  | { kind: "loading" }
  | { kind: "anon" }
  | { kind: "user"; username: string };

export default function App() {
  const [auth, setAuth] = useState<AuthState>({ kind: "loading" });

  useEffect(() => {
    api<MeResponse>("/api/auth/me")
      .then((me) => setAuth({ kind: "user", username: me.username }))
      .catch(() => setAuth({ kind: "anon" }));
  }, []);

  useEffect(() => {
    if (auth.kind !== "user") return;
    return startRefreshTimer(() => setAuth({ kind: "anon" }));
  }, [auth.kind]);

  if (auth.kind === "loading") {
    return <div className="p-8 text-neutral-500">Loading…</div>;
  }
  if (auth.kind === "anon") {
    return <AuthCard onLoggedIn={(username) => setAuth({ kind: "user", username })} />;
  }
  return (
    <Leaderboard
      username={auth.username}
      onLogout={async () => {
        await fetch("/api/auth/logout", { method: "POST" });
        setAuth({ kind: "anon" });
      }}
    />
  );
}
```

（`Leaderboard` 组件在 Task 15 实现；本任务先建最小占位 `components/Leaderboard.tsx` 导出 `({ username, onLogout }: { username: string; onLogout: () => void }) => <div>{username}</div>`，避免编译失败。）

`main.tsx` 保持模板默认（渲染 `<App />`，引入 `index.css`）。

- [ ] **Step 3: 手工验证**

前置：后端已跑（`make api`），已用 admin 建邀请码。

Run: `cd frontend && npm run dev`，浏览器开 `http://localhost:5173`
验证步骤：
1. 未登录显示登录卡片
2. 用错误邀请码注册 → 显示错误
3. 用有效邀请码注册 → 进入榜单页（显示 username）
4. 刷新页面 → 仍保持登录（me + cookie）
5. 登出 → 回到登录卡片

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(frontend): auth card and login gate with refresh timer"
```

---

### Task 15: 榜单界面 + URL 同步 + README + 端到端验收

**Files:**
- Create: `frontend/src/components/Controls.tsx`、`LeaderboardTable.tsx`
- Modify: `frontend/src/components/Leaderboard.tsx`（替换占位）
- Create: `README.md`
- Test: 手工端到端

**Interfaces:**
- Consumes: Task 13 `api`、types；后端 `/api/leaderboard/*`、`/api/languages`、`/api/meta`
- URL query 契约：`board=trending|top`、`metric=stars|forks|watchers`（仅 top 生效）、`lang=<language>`

- [ ] **Step 1: Controls.tsx**

```tsx
export type BoardKind = "trending" | "top";
export type Metric = "stars" | "forks" | "watchers";

interface Props {
  board: BoardKind;
  metric: Metric;
  language: string;
  languages: { language: string; count: number }[];
  onBoard: (b: BoardKind) => void;
  onMetric: (m: Metric) => void;
  onLanguage: (l: string) => void;
}

export default function Controls({ board, metric, language, languages, onBoard, onMetric, onLanguage }: Props) {
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-neutral-800 pb-4">
      <div className="flex gap-1 rounded-md bg-neutral-900 p-1">
        {(["trending", "top"] as BoardKind[]).map((b) => (
          <button
            key={b}
            onClick={() => onBoard(b)}
            className={`rounded px-3 py-1 text-sm ${board === b ? "bg-emerald-600 text-white" : "text-neutral-400 hover:text-neutral-200"}`}
          >
            {b === "trending" ? "趋势榜" : "总榜"}
          </button>
        ))}
      </div>

      {board === "top" && (
        <div className="flex gap-3 text-sm">
          {(["stars", "forks", "watchers"] as Metric[]).map((m) => (
            <label key={m} className="flex items-center gap-1 text-neutral-300">
              <input type="radio" name="metric" checked={metric === m} onChange={() => onMetric(m)} />
              {m === "stars" ? "Star" : m === "forks" ? "Fork" : "Watch"}
            </label>
          ))}
        </div>
      )}

      <select
        className="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-sm"
        value={language}
        onChange={(e) => onLanguage(e.target.value)}
      >
        <option value="">全部语言</option>
        {languages.map((l) => (
          <option key={l.language} value={l.language}>
            {l.language} ({l.count})
          </option>
        ))}
      </select>
    </div>
  );
}
```

- [ ] **Step 2: LeaderboardTable.tsx**

```tsx
import { compact } from "../api";
import type { LeaderboardItem } from "../types";
import type { BoardKind, Metric } from "./Controls";

interface Props {
  items: LeaderboardItem[];
  board: BoardKind;
  metric: Metric;
}

export default function LeaderboardTable({ items, board, metric }: Props) {
  if (items.length === 0) {
    return <p className="py-8 text-center text-neutral-500">暂无数据（今日抓取可能未完成）</p>;
  }
  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="border-b border-neutral-800 text-left text-neutral-500">
          <th className="py-2 pr-2 w-10">#</th>
          <th className="py-2 pr-2">Repo</th>
          <th className="py-2 pr-2">Language</th>
          {board === "trending" ? (
            <>
              <th className="py-2 pr-2 text-right">★ today</th>
              <th className="py-2 pr-2 text-right">★</th>
              <th className="py-2 text-right">Fork</th>
            </>
          ) : (
            <>
              <th className="py-2 pr-2 text-right">{metric === "stars" ? "★" : metric === "forks" ? "Fork" : "Watch"}</th>
              <th className="py-2 pr-2 text-right">★</th>
              <th className="py-2 text-right">Fork</th>
            </>
          )}
        </tr>
      </thead>
      <tbody>
        {items.map((item) => (
          <tr key={item.full_name} className="border-b border-neutral-900 hover:bg-neutral-900/50">
            <td className="py-2 pr-2 text-neutral-500">{item.rank}</td>
            <td className="py-2 pr-2">
              <a
                href={item.html_url}
                target="_blank"
                rel="noopener noreferrer"
                className="text-emerald-400 hover:underline"
                title={item.description ?? undefined}
              >
                {item.full_name}
              </a>
            </td>
            <td className="py-2 pr-2 text-neutral-400">{item.language ?? "—"}</td>
            {board === "trending" ? (
              <>
                <td className="py-2 pr-2 text-right font-medium">{compact(item.stars_today ?? 0)}</td>
                <td className="py-2 pr-2 text-right text-neutral-300">{compact(item.stars)}</td>
                <td className="py-2 text-right text-neutral-300">{compact(item.forks)}</td>
              </>
            ) : (
              <>
                <td className="py-2 pr-2 text-right font-medium">
                  {compact(metric === "stars" ? item.stars : metric === "forks" ? item.forks : item.watchers ?? 0)}
                </td>
                <td className="py-2 pr-2 text-right text-neutral-300">{compact(item.stars)}</td>
                <td className="py-2 text-right text-neutral-300">{compact(item.forks)}</td>
              </>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
```

- [ ] **Step 3: Leaderboard.tsx（数据加载 + URL 同步）**

```tsx
import { useCallback, useEffect, useState } from "react";
import Controls, { BoardKind, Metric } from "./Controls";
import LeaderboardTable from "./LeaderboardTable";
import { api } from "../api";
import type { LanguageOption, LeaderboardResponse } from "../types";

interface Props {
  username: string;
  onLogout: () => void;
}

function readUrl(): { board: BoardKind; metric: Metric; lang: string } {
  const params = new URLSearchParams(window.location.search);
  const board = params.get("board") === "top" ? "top" : "trending";
  const metricRaw = params.get("metric");
  const metric: Metric = metricRaw === "forks" || metricRaw === "watchers" ? metricRaw : "stars";
  return { board, metric, lang: params.get("lang") ?? "" };
}

export default function Leaderboard({ username, onLogout }: Props) {
  const initial = readUrl();
  const [board, setBoard] = useState<BoardKind>(initial.board);
  const [metric, setMetric] = useState<Metric>(initial.metric);
  const [lang, setLang] = useState(initial.lang);
  const [languages, setLanguages] = useState<LanguageOption[]>([]);
  const [data, setData] = useState<LeaderboardResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const params = new URLSearchParams();
    params.set("board", board);
    if (board === "top") params.set("metric", metric);
    if (lang) params.set("lang", lang);
    window.history.replaceState(null, "", `?${params.toString()}`);
  }, [board, metric, lang]);

  useEffect(() => {
    api<LanguageOption[]>("/api/languages").then(setLanguages).catch(() => {});
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    const langQuery = lang ? `&language=${encodeURIComponent(lang)}` : "";
    const path =
      board === "top"
        ? `/api/leaderboard/top?metric=${metric}${langQuery}`
        : `/api/leaderboard/trending?x=1${langQuery}`;
    try {
      setData(await api<LeaderboardResponse>(path));
    } catch (e) {
      setError(e instanceof Error ? e.message : "load failed");
    } finally {
      setLoading(false);
    }
  }, [board, metric, lang]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <div className="mx-auto max-w-5xl space-y-4 p-6">
        <header className="flex items-center justify-between">
          <h1 className="text-xl font-semibold">GH Trending</h1>
          <div className="flex items-center gap-3 text-sm text-neutral-400">
            {data?.date && <span>数据截至 {data.date}</span>}
            <span>{username}</span>
            <button onClick={onLogout} className="text-neutral-300 hover:text-white">登出</button>
          </div>
        </header>

        <Controls
          board={board}
          metric={metric}
          language={lang}
          languages={languages}
          onBoard={setBoard}
          onMetric={setMetric}
          onLanguage={setLang}
        />

        {loading && <p className="py-8 text-center text-neutral-500">加载中…</p>}
        {error && <p className="py-8 text-center text-red-400">{error}</p>}
        {!loading && !error && data && <LeaderboardTable items={data.items} board={board} metric={metric} />}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: README.md**

```markdown
# GH Trending

需登录的 GitHub 每日排行榜：趋势榜（stars today）+ 总榜（star/fork/watch top100，按语言筛选）。

## 快速开始

```bash
cp .env.example .env        # 按需修改（GITHUB_TOKEN 强烈建议填写，watch 榜必需）
export $(grep -v '^#' .env | xargs)
make db                     # 启动 PostgreSQL
make admin                  # 创建首个账号（bootstrap）
cd backend && cargo run -p ght-admin -- invite create   # 生成注册用邀请码
make collect                # 首次抓取（需网络；无 token 时 watch 榜自动跳过）
make api                    # 启动 API :8000
make web                    # 前端 dev server :5173
```

打开 http://localhost:5173，用邀请码注册后登录。

## 数据口径说明

- 趋势榜：来自 github.com/trending 页面官方口径，每语言约 25 条，仅 star 增量。
- 总榜 star/fork：GitHub Search API 按语言取 top100。
- 总榜 watch：候选池为 star top500，GraphQL 批量查 subscribers 后取 top100；
  因此 watch top100 限定在 star top500 范围内（watch 与 star 强相关，偏差极小）。
- 所有榜单数据按天快照存档，当日重复抓取幂等。

## 架构

见 docs/superpowers/specs/2026-08-06-github-leaderboard-design.md。

## 常用命令

| 命令 | 说明 |
|---|---|
| make db / make db-down | 起/停 PostgreSQL |
| make collect | collector 单次抓取 |
| make api | 启动 API |
| make web | 前端 dev server |
| make test | 全量测试（cargo test + vitest） |
```

- [ ] **Step 5: 端到端手工验收**

前置：`.env` 配好（含有效 `GITHUB_TOKEN`）、`make db` 已运行。

1. `cargo run -p ght-admin -- create-user --username admin --password <pw>` 建首账号
2. `cargo run -p ght-admin -- invite create` 拿邀请码
3. `make collect`：日志显示 search/graphql/trending 各阶段完成，exit 0
4. `make api` + `make web`，浏览器验证：
   - 未登录 → 登录卡片；错误邀请码注册 → 报错
   - 有效邀请码注册 → 进入榜单；趋势榜有 ★ today 列；总榜三指标切换数据变化
   - 语言筛选后排名重算（#1 变化）；刷新页面筛选保持（URL 同步）
   - 点击 repo 名称新标签页打开 GitHub
   - 登出 → 回到登录卡片
5. `make test` 全绿。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(frontend): leaderboard ui with url-synced filters, readme, e2e verified"
```

---

## 完成标准（Definition of Done）

- `cargo test`（workspace）与 `npm test` 全部通过
- `make collect` 真实抓取一次成功（有 token 时四榜齐全；无 token 时 watch 榜降级且其余正常）
- 手工验收 Step 5 全部通过
- 数据库无外键（`\d snapshots` 等确认无 FK 约束）
- 请求路径零查库：受保护接口只验 JWT 签名（代码审查确认 `RequireAuth` 无 DB 调用）
