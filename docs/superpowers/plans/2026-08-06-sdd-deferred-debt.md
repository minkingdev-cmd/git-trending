# SDD Deferred 技术债偿还 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 清零 Task 1–7 SDD review 中全部 deferred minor / hardening notes（D1–D9），不改 API / schema / 前端。

**Architecture:** 纯后端小改：workspace 依赖整理、config/store 健壮性与缺测、collector 客户端（search/graphql/trending）限速与日志、collect 幂等断言增强、daemon 优雅 shutdown。全语言 Search 查询串改为 `is:public`。

**Tech Stack:** Rust 2021 workspace、sqlx、reqwest、scraper、tokio-cron-scheduler 0.10、wiremock、serial_test、tracing。

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-06-sdd-deferred-debt-design.md`
- 无 HTTP API / DB migration / 前端变更
- 测试：`export RUSTUP_TOOLCHAIN=stable`；`DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending`；crate 测试库 `DATABASE_URL_TEST` / `_API` / `_COLLECTOR`
- 提交信息 conventional commits（`fix:` / `test:` / `chore:`）
- GraphQL 批间 sleep `1500ms`；最后一批后不 sleep
- Search `lang=None` → `q=is:public`（已确认）
- 接受残留：多字节 collect_time 切片、纯 mid-module `use` 风格不单开任务

## File Structure

```
backend/
  Cargo.toml                          # + serial_test workspace dep
  crates/
    core/
      Cargo.toml                      # serial_test = { workspace = true }
      src/config.rs                   # collect_time_parts no unwrap + tests
      src/store.rs                    # board_count / cleanup tests
    api/Cargo.toml                    # serial_test workspace
    collector/
      Cargo.toml                      # serial_test workspace
      src/search.rs                   # per_page hoist, is:public
      src/graphql.rs                  # warn + batch sleep
      src/trending.rs                 # p.col-9, debug skip, lang=None test
      src/collect.rs                  # multi-board idempotency asserts
      src/main.rs                     # mut scheduler + shutdown
      tests/fixtures/trending.html    # already has col-9; verify only
```

---

### Task 1: Workspace serial_test（D6）

**Files:**
- Modify: `backend/Cargo.toml`
- Modify: `backend/crates/core/Cargo.toml`
- Modify: `backend/crates/api/Cargo.toml`
- Modify: `backend/crates/collector/Cargo.toml`

**Interfaces:**
- Produces: workspace dep `serial_test = "3"`；各 crate `serial_test = { workspace = true }`

- [ ] **Step 1: 写入 workspace 依赖并改各 crate**

`backend/Cargo.toml` 在 `[workspace.dependencies]` 末尾加：

```toml
serial_test = "3"
```

三处 dev-dependencies 改为：

```toml
serial_test = { workspace = true }
```

（core / api / collector 各自 `Cargo.toml` 中原 `serial_test = "3"` 替换。）

- [ ] **Step 2: 编译确认**

Run:

```bash
export RUSTUP_TOOLCHAIN=stable
export DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
cd backend && cargo check -p ght-core -p ght-api -p ght-collector
```

Expected: 成功（无 unresolved serial_test）。

- [ ] **Step 3: Commit**

```bash
git add backend/Cargo.toml backend/crates/core/Cargo.toml backend/crates/api/Cargo.toml backend/crates/collector/Cargo.toml backend/Cargo.lock
git commit -m "chore: hoist serial_test to workspace dependencies"
```

---

### Task 2: Config collect_time_parts 健壮性（D4）

**Files:**
- Modify: `backend/crates/core/src/config.rs`

**Interfaces:**
- Produces: `collect_time_parts(t: &str) -> Result<(u32, u32), ConfigError>` 无 unwrap

- [ ] **Step 1: 写失败测试（边界）**

在 `config.rs` 的 `#[cfg(test)] mod tests` 追加：

```rust
#[test]
fn collect_time_parts_boundary_ok() {
    assert_eq!(collect_time_parts("00:00").unwrap(), (0, 0));
    assert_eq!(collect_time_parts("23:59").unwrap(), (23, 59));
}

#[test]
fn collect_time_parts_rejects_invalid() {
    for bad in ["24:00", "12:60", "", "9:00", "0900"] {
        assert!(
            collect_time_parts(bad).is_err(),
            "expected err for {bad:?}"
        );
    }
}
```

（保留已有 `collect_time_parts_splits_hh_mm`。）

- [ ] **Step 2: 运行确认（实现仍用 unwrap 时也应 PASS；若已有实现则直接 PASS）**

Run: `cd backend && cargo test -p ght-core collect_time_parts -- --nocapture`  
Expected: PASS（当前校验已挡非法串；下一步去掉 unwrap）

- [ ] **Step 3: 实现无 unwrap 解析**

替换 `collect_time_parts` 为：

```rust
pub fn collect_time_parts(t: &str) -> Result<(u32, u32), ConfigError> {
    validate_collect_time(t)?;
    let hour: u32 = t[..2]
        .parse()
        .map_err(|_| ConfigError::BadCollectTime(t.to_string()))?;
    let minute: u32 = t[3..]
        .parse()
        .map_err(|_| ConfigError::BadCollectTime(t.to_string()))?;
    Ok((hour, minute))
}
```

- [ ] **Step 4: 再跑 config 测试**

Run: `cd backend && cargo test -p ght-core config`  
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/core/src/config.rs
git commit -m "fix(core): collect_time_parts without unwrap and boundary tests"
```

---

### Task 3: Store 缺测 board_count + cleanup（D5）

**Files:**
- Modify: `backend/crates/core/src/store.rs`（`#[cfg(test)]` 模块）

**Interfaces:**
- Consumes: `board_count`, `cleanup_expired_refresh_tokens`, 既有 `upsert_*` / `test_pool` 模式
- Produces: 两个新 `#[tokio::test] #[serial]` 测试

- [ ] **Step 1: 追加 board_count 测试**

在 `store.rs` tests 中追加（使用唯一 full_name 前缀 `bcnt/`）：

```rust
#[tokio::test]
#[serial]
async fn board_count_counts_rows_for_board_and_date() {
    let pool = test_pool().await;
    let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
    for (name, stars) in [("bcnt/a", 10), ("bcnt/b", 20)] {
        let id = upsert_repo(&pool, &repo(name, Some("Rust")), date).await.unwrap();
        upsert_snapshot(
            &pool,
            id,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars,
                forks: 0,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();
    }
    // 另一 board 不应计入 TopStars
    let id = upsert_repo(&pool, &repo("bcnt/c", None), date).await.unwrap();
    upsert_snapshot(
        &pool,
        id,
        date,
        Board::TopForks,
        &SnapshotInput {
            stars: 1,
            forks: 9,
            watchers: None,
            stars_today: None,
        },
    )
    .await
    .unwrap();

    let n = board_count(&pool, date, Board::TopStars).await.unwrap();
    // 可能含历史 bcnt 残留时用 >=2 并过滤：干净做法先删本前缀
    assert!(n >= 2);
    let rows = top_by_stars(&pool, date, None, 1000).await.unwrap();
    let mine = rows.iter().filter(|r| r.full_name.starts_with("bcnt/")).count();
    assert_eq!(mine, 2);
    assert_eq!(
        board_count(&pool, date, Board::TopForks).await.unwrap()
            >= 1,
        true
    );
}
```

更干净的版本（推荐写入计划实现时采用）：测试开头

```rust
sqlx::query("DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'bcnt/%')")
    .execute(&pool).await.unwrap();
sqlx::query("DELETE FROM repos WHERE full_name LIKE 'bcnt/%'")
    .execute(&pool).await.unwrap();
```

然后 `assert_eq!(board_count(... TopStars), 2)` 且 `assert_eq!(board_count(... TopForks), 1)`。

- [ ] **Step 2: 追加 cleanup_expired 测试**

```rust
#[tokio::test]
#[serial]
async fn cleanup_expired_refresh_tokens_only_deletes_expired() {
    let pool = test_pool().await;
    sqlx::query("TRUNCATE refresh_tokens, users CASCADE").execute(&pool).await.ok();
    // 无 FK；用 TRUNCATE users, invite_codes, refresh_tokens
    sqlx::query("TRUNCATE users, invite_codes, refresh_tokens")
        .execute(&pool)
        .await
        .unwrap();

    let uid: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ('cleanup_u', 'h') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, 'h_old', now() - interval '1 day')",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, 'h_new', now() + interval '1 day')",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();

    let deleted = cleanup_expired_refresh_tokens(&pool).await.unwrap();
    assert_eq!(deleted, 1);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 1);
}
```

注意：`test_pool` 当前可能不 TRUNCATE；本测试自行 TRUNCATE 用户相关表即可。

- [ ] **Step 3: 运行 store 测试**

Run:

```bash
export RUSTUP_TOOLCHAIN=stable DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
export DATABASE_URL_TEST=postgres://ght:ght@localhost:5433/ghtrending_test
cd backend && cargo test -p ght-core store -- --nocapture
```

Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add backend/crates/core/src/store.rs
git commit -m "test(core): cover board_count and cleanup_expired_refresh_tokens"
```

---

### Task 4: Search is:public + per_page 提升（D7）

**Files:**
- Modify: `backend/crates/collector/src/search.rs`

**Interfaces:**
- Produces: `search_top(..., lang: None, ...)` 发送 `q=is:public`

- [ ] **Step 1: 更新/新增失败测试**

在 `paginates_until_empty_page` 或新增测试中，对 `lang=None` 断言 `query_param("q", "is:public")`：

```rust
#[tokio::test]
async fn none_lang_uses_is_public_query() {
    use wiremock::matchers::query_param;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "is:public"))
        .and(query_param("sort", "stars"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&["a/x"])))
        .expect(1)
        .mount(&server)
        .await;
    let client = reqwest::Client::new();
    let repos = search_top(&client, &server.uri(), None, None, Metric::Stars, 100, 1)
        .await
        .unwrap();
    assert_eq!(repos.len(), 1);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test -p ght-collector none_lang_uses_is_public`  
Expected: FAIL（当前空 q 不匹配 `is:public`）

- [ ] **Step 3: 实现**

在 `search_top` 内：

```rust
let q = match lang {
    Some(l) => format!("language:{l}"),
    None => "is:public".to_string(),
};
let per_page_s = per_page.to_string();
for page in 1..=pages {
    let page_s = page.to_string();
    let mut req = client
        .get(format!("{base}/search/repositories"))
        .header("Accept", "application/vnd.github+json")
        .query(&[
            ("q", q.as_str()),
            ("sort", metric.as_str()),
            ("per_page", per_page_s.as_str()),
            ("page", page_s.as_str()),
        ]);
    // ... rest unchanged
}
```

- [ ] **Step 4: 运行 collector search 测试**

Run: `cd backend && cargo test -p ght-collector search`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/collector/src/search.rs
git commit -m "fix(collector): search q=is:public for all-language queries"
```

---

### Task 5: GraphQL warn + 批间 sleep（D2, D3）

**Files:**
- Modify: `backend/crates/collector/src/graphql.rs`

**Interfaces:**
- Produces: `GRAPHQL_INTERVAL = 1500ms`；`build_watchers_query` 对含 `"` 的 target warn 并跳过；`fetch_watchers` 批间 sleep（最后一批不 sleep）

- [ ] **Step 1: 写失败测试（quote 跳过）**

```rust
#[test]
fn build_skips_targets_with_quotes_in_names() {
    let q = build_watchers_query(&[
        WatchTarget { owner: "a".into(), name: "x".into() },
        WatchTarget { owner: "bad\"one".into(), name: "x".into() },
        WatchTarget { owner: "b".into(), name: "y".into() },
    ]);
    assert!(q.contains(r#"repository(owner: "a", name: "x")"#));
    assert!(q.contains(r#"repository(owner: "b", name: "y")"#));
    assert!(!q.contains("bad"));
}
```

（现有实现已跳过 quote；本测试锁定行为。实现 step 补 warn。）

- [ ] **Step 2: 实现 warn + sleep**

在 `build_watchers_query` 的 filter 分支改为显式循环或 `inspect`：

```rust
.filter(|(_, t)| {
    let ok = !t.owner.contains('"') && !t.name.contains('"');
    if !ok {
        tracing::warn!(owner = %t.owner, name = %t.name, "skipping watch target with quote in name");
    }
    ok
})
```

注意：`enumerate` 在 filter 之后会重编号 alias（q0,q1…）——**保持当前 filter 再 map 的顺序**，alias 连续，与 `parse_watchers` 按 batch 下标对齐。当前代码是 `enumerate().filter(...).map(...)`，filter 后 index 可能不连续（q0, q2）！检查现有：

```rust
.enumerate()
.filter(|(_, t)| !t.owner.contains('"') && !t.name.contains('"'))
.map(|(i, t)| format!(r#"q{i}: ..."#))
```

这会导致 parse 时用 batch 原始下标对不上。**本任务必须修为：先过滤成干净 batch 再 enumerate**，或 parse 时只遍历出现的 alias。

**正确实现：**

```rust
pub fn build_watchers_query(batch: &[WatchTarget]) -> String {
    let clean: Vec<&WatchTarget> = batch
        .iter()
        .filter(|t| {
            let ok = !t.owner.contains('"') && !t.name.contains('"');
            if !ok {
                tracing::warn!(owner = %t.owner, name = %t.name, "skipping watch target with quote in name");
            }
            ok
        })
        .collect();
    let fields: Vec<String> = clean
        .iter()
        .enumerate()
        .map(|(i, t)| {
            format!(
                r#"q{i}: repository(owner: "{}", name: "{}") {{ watchers {{ totalCount }} }}"#,
                t.owner, t.name
            )
        })
        .collect();
    format!("query {{ {} }}", fields.join(" "))
}
```

**同步 `parse_watchers`：** 必须只对 clean targets 解析，或 `fetch_watchers` 对每个 chunk 先 filter 再 build/parse。推荐在 `fetch_watchers` 内：

```rust
for (idx, chunk) in targets.chunks(batch_size.max(1)).enumerate() {
    let clean: Vec<WatchTarget> = chunk.iter().filter(|t| {
        let ok = !t.owner.contains('"') && !t.name.contains('"');
        if !ok {
            tracing::warn!(owner = %t.owner, name = %t.name, "skipping watch target with quote in name");
        }
        ok
    }).cloned().collect();
    if clean.is_empty() { continue; }
    // post build_watchers_query(&clean) — 若 build 不再 filter 则只 build clean
    ...
    map.extend(parse_watchers(&resp, &clean));
    if /* not last non-empty chunk */ {
        // sleep only if more chunks remain
    }
}
```

为简单起见：**build 保持只负责字符串；filter+warn 集中在 fetch_watchers**；`build_watchers_query` 假定输入已 clean（仍做防御性 filter+warn）。`parse_watchers` 的 batch 与 query alias 使用同一 clean 切片。

Sleep：

```rust
use std::time::Duration;
pub const GRAPHQL_INTERVAL: Duration = Duration::from_millis(1500);

// after successful parse of chunk i, if more chunks:
tokio::time::sleep(GRAPHQL_INTERVAL).await;
```

- [ ] **Step 3: 运行 graphql 测试**

Run: `cd backend && cargo test -p ght-collector graphql`  
Expected: PASS（`fetch_batches` 会多等约 1.5s）

- [ ] **Step 4: Commit**

```bash
git add backend/crates/collector/src/graphql.rs
git commit -m "fix(collector): graphql batch pacing and warn on quoted names"
```

---

### Task 6: Trending 选择器与 lang=None fetch（D8）

**Files:**
- Modify: `backend/crates/collector/src/trending.rs`
- Verify: `backend/crates/collector/tests/fixtures/trending.html`（已有 `p class="col-9 ..."`）

**Interfaces:**
- Produces: `desc_sel = p.col-9`；缺 stars/forks 时 debug 日志后 skip；`fetch_trending(..., None)` 测 `/trending?since=daily`

- [ ] **Step 1: 改 description 选择器并加 skip 日志**

```rust
let desc_sel = Selector::parse("p.col-9").unwrap();
// in filter_map, when stars/forks missing:
// after full_name known:
let stars = match row.select(&stars_sel).next().and_then(|el| parse_count(...)) {
    Some(s) => s,
    None => {
        tracing::debug!(%full_name, "trending row missing stars; skipping");
        return None;
    }
};
// same for forks
```

- [ ] **Step 2: 追加 fetch lang=None 测试**

```rust
#[tokio::test]
async fn fetch_all_languages_path() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/trending"))
        .and(query_param("since", "daily"))
        .and(header("user-agent", "gh-trending-collector/0.1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(include_str!("../tests/fixtures/trending.html")))
        .expect(1)
        .mount(&server)
        .await;
    let client = reqwest::Client::builder()
        .user_agent("gh-trending-collector/0.1")
        .build()
        .unwrap();
    let repos = fetch_trending(&client, &server.uri(), None).await.unwrap();
    assert_eq!(repos.len(), 3);
}
```

（`header` matcher 需 `use wiremock::matchers::header`。）

- [ ] **Step 3: 运行 trending 测试**

Run: `cd backend && cargo test -p ght-collector trending`  
Expected: PASS（fixture 仍解析 3 行）

- [ ] **Step 4: Commit**

```bash
git add backend/crates/collector/src/trending.rs
git commit -m "fix(collector): tighter trending desc selector and all-lang fetch test"
```

---

### Task 7: Collect 多 board 幂等断言（D9）

**Files:**
- Modify: `backend/crates/collector/src/collect.rs` tests

**Interfaces:**
- Consumes: `core_store::board_count` 或现有 `top_by_*` / `trending` len

- [ ] **Step 1: 强化 idempotency 断言**

在 `collect_once_writes_all_boards_and_is_idempotent` 第二次 collect 后替换/扩展为：

```rust
let report2 = collector.collect_once().await;
assert_eq!(report2.failed, 0);
assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
assert_eq!(core_store::top_by_forks(&pool, today, None, 100).await.unwrap().len(), 1);
assert_eq!(core_store::top_by_watchers(&pool, today, None, 100).await.unwrap().len(), 1);
assert_eq!(core_store::trending(&pool, today, None, 100).await.unwrap().len(), 3);
assert_eq!(
    core_store::board_count(&pool, today, Board::TopStars).await.unwrap(),
    1
);
assert_eq!(
    core_store::board_count(&pool, today, Board::TopForks).await.unwrap(),
    1
);
assert_eq!(
    core_store::board_count(&pool, today, Board::TopWatchers).await.unwrap(),
    1
);
assert_eq!(
    core_store::board_count(&pool, today, Board::TrendingDaily).await.unwrap(),
    3
);
```

需 `use ght_core::models::Board;`。

- [ ] **Step 2: 运行 collect 集成测试**

Run:

```bash
export DATABASE_URL_TEST_COLLECTOR=postgres://ght:ght@localhost:5433/ghtrending_test_collector
export DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
export RUSTUP_TOOLCHAIN=stable
cd backend && cargo test -p ght-collector collect -- --nocapture
```

Expected: PASS（较慢，含 sleep）

- [ ] **Step 3: Commit**

```bash
git add backend/crates/collector/src/collect.rs
git commit -m "test(collector): assert multi-board idempotency after recollect"
```

---

### Task 8: Collector daemon shutdown（D1）

**Files:**
- Modify: `backend/crates/collector/src/main.rs`

**Interfaces:**
- Consumes: `JobScheduler::shutdown(&mut self) -> Result<(), JobSchedulerError>`（tokio-cron-scheduler 0.10）

- [ ] **Step 1: 实现 mut scheduler + shutdown**

```rust
let mut scheduler = JobScheduler::new().await?;
// ... add job, start ...
tokio::signal::ctrl_c().await?;
tracing::info!("collector shutting down");
if let Err(e) = scheduler.shutdown().await {
    tracing::warn!(error = %e, "scheduler shutdown failed");
}
Ok(())
```

- [ ] **Step 2: 编译**

Run: `cd backend && cargo build -p ght-collector`  
Expected: 成功

- [ ] **Step 3: Commit**

```bash
git add backend/crates/collector/src/main.rs
git commit -m "fix(collector): graceful JobScheduler shutdown on ctrl-c"
```

---

### Task 9: 全量验证

**Files:** 无新文件（若 sqlx 缓存变化则更新 `.sqlx/`）

- [ ] **Step 1: Workspace 测试**

```bash
export RUSTUP_TOOLCHAIN=stable
export DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
export DATABASE_URL_TEST=postgres://ght:ght@localhost:5433/ghtrending_test
export DATABASE_URL_TEST_API=postgres://ght:ght@localhost:5433/ghtrending_test_api
export DATABASE_URL_TEST_COLLECTOR=postgres://ght:ght@localhost:5433/ghtrending_test_collector
export JWT_SECRET=change-me
cd backend && cargo test --workspace
```

Expected: 全部 crate PASS

- [ ] **Step 2: 更新 SDD ledger（本地 .superpowers 可写 progress 段落）**

在 progress 记：`Deferred debt plan complete (D1–D9)`。

- [ ] **Step 3: 最终 commit（若有 sqlx 或文档）**

```bash
git status
# 若有 .sqlx 变更：
# git add backend/.sqlx && git commit -m "chore: refresh sqlx offline cache after deferred tests"
```

---

## Spec coverage checklist

| Spec ID | Task |
|---------|------|
| D1 shutdown | Task 8 |
| D2 GraphQL sleep | Task 5 |
| D3 quote warn | Task 5 |
| D4 collect_time_parts | Task 2 |
| D5 board_count/cleanup tests | Task 3 |
| D6 serial_test workspace | Task 1 |
| D7 is:public + per_page | Task 4 |
| D8 trending | Task 6 |
| D9 multi-board idempotency | Task 7 |
| Accepted residuals | documented in spec; no task |
| Full verify | Task 9 |

## Placeholder scan

无 TBD / “implement later” / 空测试步骤。
