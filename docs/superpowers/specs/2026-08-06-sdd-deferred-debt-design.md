# SDD Deferred 技术债偿还 — 设计文档

日期：2026-08-06  
状态：已确认（brainstorming 定稿，Approach B + `q=is:public`）  
关联：`docs/superpowers/plans/2026-08-06-github-leaderboard.md` Task 1–7 review deferred 项

## 1. 目标

清零 Task 1–7 SDD review 中全部 **deferred minor / hardening notes**，补齐对应测试。不引入产品功能、不改 HTTP API 契约、不改 DB schema、不改前端。

### 成功标准

- 下文清单每一项有代码改动，或在本文「接受的残留」中写明理由
- `RUSTUP_TOOLCHAIN=stable cargo test --workspace` 全绿（DATABASE_URL 指向本机 PG）
- 现有 fixture / wiremock 契约不被无意破坏

### 非目标

- 可观测性产品（metrics / 抓取状态表）
- 安全限流 / 管理后台新能力
- 真实 GitHub 页面回归抓取（仅 fixture + wiremock）

## 2. 范围清单

| ID | 来源 | 改动 |
|----|------|------|
| D1 | Task 7 | daemon `ctrl_c` 后 `scheduler.shutdown()` |
| D2 | Task 6 note | GraphQL 批间 sleep（1.5s） |
| D3 | Task 5 | quote 过滤 target 时 `tracing::warn!` |
| D4 | Task 7 | `collect_time_parts` 去 `unwrap` + 边界测试 |
| D5 | Task 2 | `board_count` / `cleanup_expired_refresh_tokens` 单测 |
| D6 | Task 6 | `serial_test` 提升为 workspace dependency |
| D7 | Task 4 | search：`per_page` 字符串循环外计算；`lang=None` 时 `q=is:public` |
| D8 | Task 3 | trending：更窄 description 选择器；缺 stars/forks 行 debug/warn；`lang=None` fetch 测试 |
| D9 | Task 6 | collect 幂等测试覆盖多 board（stars/forks/trending；有 token 时含 watch） |

### 接受的残留（不改代码）

| 项 | 理由 |
|----|------|
| `validate_collect_time` 多字节 panic | `len()==5` 且要求 ascii digit，非法 UTF-8 切片路径不可达 |
| mid-module `use` 纯风格 | 非功能；仅在触及相关文件时顺手整理，不单独开任务 |

## 3. 详细设计

### 3.1 Collector daemon 关闭（D1）

**文件：** `backend/crates/collector/src/main.rs`

常驻路径在 `tokio::signal::ctrl_c().await?` 之后：

```text
tracing::info!(...);
if let Err(e) = scheduler.shutdown().await {
    tracing::warn!(error = %e, "scheduler shutdown failed");
}
Ok(())
```

使用 `tokio-cron-scheduler` 的 `JobScheduler::shutdown`（若 API 为 `shutdown()` 无返回值则直接 await）。不因 shutdown 错误导致进程非零退出，保证信号路径可预期结束。

### 3.2 GraphQL 批间限速与过滤日志（D2, D3）

**文件：** `backend/crates/collector/src/graphql.rs`

- 常量 `GRAPHQL_INTERVAL = Duration::from_millis(1500)`（与 Search 间隔对齐）
- `fetch_watchers`：每个 chunk 请求完成后、进入下一 chunk 前 `tokio::time::sleep(GRAPHQL_INTERVAL).await`（最后一批后可不 sleep，避免无意义等待）
- `build_watchers_query`：过滤含 `"` 的 owner/name 时 `tracing::warn!(owner, name, "skipping watch target with quote in name")`
- 测试：构造含 `"` 的 target 与干净 target 的 batch；解析/查询字符串不含脏 alias；干净 target 仍出现在 query 中

### 3.3 Config 健壮性（D4）

**文件：** `backend/crates/core/src/config.rs`

```rust
pub fn collect_time_parts(t: &str) -> Result<(u32, u32), ConfigError> {
    validate_collect_time(t)?;
    let hour: u32 = t[..2].parse().map_err(|_| ConfigError::BadCollectTime(t.to_string()))?;
    let minute: u32 = t[3..].parse().map_err(|_| ConfigError::BadCollectTime(t.to_string()))?;
    Ok((hour, minute))
}
```

**测试追加：**

- `00:00` → `(0, 0)`；`23:59` → `(23, 59)`
- `24:00`、`12:60`、`""`、`9:00`、`0900` → err

### 3.4 Store 缺测（D5）

**文件：** `backend/crates/core/src/store.rs` tests

- `board_count_counts_rows_for_board_and_date`：插入 2 行 TopStars 同日，断言 `board_count == 2`；另一 board 不计入
- `cleanup_expired_refresh_tokens_only_deletes_expired`：插入 user + 两条 refresh（一条 `expires_at` 过去、一条未来），调用 cleanup，断言只剩 1 行

使用现有 per-crate test DB + `serial_test` 模式。

### 3.5 Workspace serial_test（D6）

**文件：**

- `backend/Cargo.toml`：`serial_test = "3"` 写入 `[workspace.dependencies]`
- `backend/crates/{core,api,collector}/Cargo.toml`：`serial_test = { workspace = true }`

### 3.6 Search 客户端（D7）

**文件：** `backend/crates/collector/src/search.rs`

- `let per_page_s = per_page.to_string();` 提到循环外；query 使用 `per_page_s.as_str()`
- `lang` 分支：

```rust
let q = match lang {
    Some(l) => format!("language:{l}"),
    None => "is:public".to_string(),
};
```

**测试：**

- 现有 `language:Python` 用例保持
- 新增或调整：`lang=None` 时 wiremock 匹配 `q=is:public`
- `paginates_until_empty_page` 等不传 lang 的用例更新 query 期望

### 3.7 Trending 解析与 fetch（D8）

**文件：** `trending.rs`、`tests/fixtures/trending.html`

- description 选择器：`p.col-9`（与当前 fixture class 一致：`p class="col-9 color-fg-muted ..."`）
- 若某 row 有 href 但缺 stargazers/forks 链接：`tracing::debug!`（或 warn）后 `filter_map` 跳过（行为与现网一致，仅加可观测性）
- 测试：
  - fixture 三行仍全部解析（第三条无 today 仍可解析）
  - `fetch_trending(&client, base, None)` mock `GET /trending?since=daily`

### 3.8 Collect 幂等（D9）

**文件：** `backend/crates/collector/src/collect.rs` 集成测试

在 `collect_once_writes_all_boards_and_is_idempotent`（或等价测试）中，第二次 `collect_once` 后：

- 对 `top_stars`、`top_forks`、`trending_daily` 分别 `board_count`（或 store 查询）断言与第一次后相同
- 若测试路径带 token / mock watch：`top_watchers` 同样断言

不改变 `collect_once` 业务逻辑，仅强化断言。

## 4. 行为影响

| 场景 | 变化 |
|------|------|
| 全语言 Search | `q` 从空串变为 `is:public`；排序/分页不变 |
| Watch 抓取耗时 | 每 GraphQL 批增加约 1.5s 间隔（最后一批除外） |
| 进程信号退出 | 尝试优雅停止 cron scheduler |
| API / DB / 前端 | **无变化** |

## 5. 错误处理

- GraphQL 过滤与 trending 跳过行：只记日志，不 fail 整天
- scheduler shutdown 失败：`warn` 后仍 `Ok(())` 退出
- config 非法时间：`ConfigError::BadCollectTime`（与现有一致）

## 6. 测试与验证

```bash
export RUSTUP_TOOLCHAIN=stable
export DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
# 及 TEST / TEST_API / TEST_COLLECTOR 环境变量
cd backend && cargo test --workspace
```

手工（可选）：`cargo run -p ght-collector` → Ctrl-C → 进程退出且无 hang。

若改动了 `query!` 相关 SQL 测试路径，按需 `cargo sqlx prepare --workspace` 更新 `.sqlx/`。

## 7. 实施顺序

1. D6 workspace serial_test  
2. D4 config  
3. D5 store tests  
4. D7 search  
5. D2–D3 graphql  
6. D8 trending  
7. D9 collect tests  
8. D1 main shutdown  
9. 全量 workspace test  

## 8. 风险

- **`is:public` 与 GitHub Search 语义**：官方文档支持；比空 `q` 更明确。若未来 GitHub 变更，仅影响全语言榜候选池，有语言时仍 `language:{lang}`。
- **trending 选择器 `p.col-9`**：与当前 fixture 对齐；若真实页面 class 变更，属于既有爬虫脆弱性，本债不扩大为多 selector 兼容矩阵。
