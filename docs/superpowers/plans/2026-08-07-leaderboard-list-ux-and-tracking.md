# Leaderboard List UX + Search + User Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把公开榜列表做成可扫读、可主题/关键词/多语言筛选的数据密集 UI（含 Light/Dark），并让登录用户能跟踪爬虫未覆盖的公开仓库。

**Architecture:** 扩展既有 `repos`/`snapshots` 与 axum 只读 API：repos 增加 `topics`/`languages`/`language_names`；榜单查询支持 `q`/`topics`/`topic_mode`/`languages` 并在服务端过滤后重算 rank；用户跟踪表 `user_tracked_repos` + 快照 board `tracked_daily` 与公开榜隔离；collector 在现有抓取后补全 enrichment 并扫描跟踪集；前端按原型 `docs/prototypes/leaderboard-v2.html` 改造 Controls/Table/主题/添加仓库/我的跟踪。

**Tech Stack:** 既有栈 — Rust 2021、axum、SQLx(postgres)、reqwest、React 19 + Vite + Tailwind 3 + vitest。无新 UI 组件库、无 react-router。

**Spec:** `docs/superpowers/specs/2026-08-07-leaderboard-list-ux-and-tracking-design.md`  
**Prototype:** `docs/prototypes/leaderboard-v2.html`

## Global Constraints

- 沿用项目规则：无跨表 `FOREIGN KEY`；`sqlx::query!` 需本地 PG（local-debug，**禁止** docker 起 PG，见 `CLAUDE.md`）；`SQLX_OFFLINE` + `.sqlx/` 缓存可提交。
- 测试库：`DATABASE_URL_TEST` 或项目 Makefile/`make test` 约定；测试永不打真实 GitHub，collector/API 集成用 wiremock。
- 公开榜 **不得** 因用户 track 插入 top100 名次；跟踪仓用 board=`tracked_daily`。
- topics 入库 lowercase + 去重；语言筛选语义 **OR**；topics 多选默认 **AND**。
- 趋势列表 **无** 列内 sparkline；历史仅 📈 弹层。
- 提交信息 conventional commits（`feat:` / `test:` / `chore:` / `docs:`）。
- 前端不引入 Ant Design / 重型 Table；主题键 `ght-theme`（`light`|`dark`）。
- 每用户跟踪上限 **50**；track 接口限流建议 ≥ 10 次/小时/用户（实现可用简单内存或 DB 计数，v1 至少 enforce 上限）。
- UI 对照原型行为，不必像素级复制 emoji（📈 可用文字「趋势」）。

---

## File Structure

```
backend/
  migrations/
    0002_repo_enrichment.sql          # topics, languages, language_names, last_enriched_at
    0003_user_tracked_repos.sql       # user_tracked_repos
  crates/core/src/
    models.rs                         # Board::TrackedDaily; RepoInput/LeaderboardRow 扩展; LanguageShare
    store.rs                          # upsert enrichment; filtered leaderboard; track CRUD; tracked list
  crates/collector/src/
    search.rs                         # 解析 topics（若 Search 返回）
    enrich.rs                         # 新建：languages + topics 批补（REST/GraphQL）
    collect.rs                        # 调用 enrich；扫描 user_tracked 写 tracked_daily
    store.rs                          # 透传 enrichment 字段到 core
  crates/api/src/
    routes_leaderboard.rs             # q/topics/languages 参数与响应
    routes_track.rs                   # 新建：lookup/track/untrack/list
    routes_history.rs                 # 允许 tracked 仓读 history
    lib.rs                            # merge routes_track
frontend/src/
  types.ts
  api.ts
  index.css                           # 主题 CSS 变量
  components/
    Leaderboard.tsx                   # URL 状态、主题、三视图、加载
    Controls.tsx                      # q/topics/languages/密度/描述/board
    LeaderboardTable.tsx              # 新列模型、语言展开
    AddRepoModal.tsx                  # 新建
    TrackedPanel.tsx                  # 新建
    RepoHistoryPanel.tsx              # 兼容跟踪仓
docs/prototypes/leaderboard-v2.html   # 已有，实现时对照，不必改
```

---

### Task 1: Migration — repo enrichment

**Files:**
- Create: `backend/migrations/0002_repo_enrichment.sql`
- Test: 迁移可被 `sqlx migrate` / API 启动自动 migrate 应用

**Interfaces:**
- Produces: columns on `repos`:
  - `topics TEXT[] NOT NULL DEFAULT '{}'`
  - `languages JSONB NOT NULL DEFAULT '[]'`
  - `language_names TEXT[] NOT NULL DEFAULT '{}'`
  - `last_enriched_at TIMESTAMPTZ`
  - GIN indexes on `topics`, `language_names`
  - optional: `CREATE EXTENSION IF NOT EXISTS pg_trgm` + trgm indexes on `full_name`, `description`

- [ ] **Step 1: 写迁移文件**

```sql
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
```

- [ ] **Step 2: 对本地 dev/test 库跑迁移**

Run（按项目 Makefile / local-debug 栈）:

```bash
# 确保本机 PG 可用，DATABASE_URL 指向 ghtrending
cd backend && sqlx migrate run
```

Expected: 成功；`\d repos` 可见新列。

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/0002_repo_enrichment.sql
git commit -m "feat(db): add repos topics and languages columns"
```

---

### Task 2: Core models + upsert + filtered leaderboard queries

**Files:**
- Modify: `backend/crates/core/src/models.rs`
- Modify: `backend/crates/core/src/store.rs`
- Test: `backend/crates/core` 内 `#[cfg(test)]` 或 api 集成测（本任务至少 unit/store 级）

**Interfaces:**
- Produces:
  - `LanguageShare { name: String, pct: f64 }`（序列化用；DB JSON 可存 name/pct/bytes）
  - `RepoInput` 增加 `topics: Vec<String>`, `languages_json: serde_json::Value`（或 `Vec<LanguageShare>`）, `language_names: Vec<String>`
  - `LeaderboardRow` 增加 `topics: Vec<String>`, `languages: serde_json::Value`（或 typed）, 保留 `language: Option<String>`
  - `Board::TrackedDaily` → `"tracked_daily"`
  - `upsert_repo` 更新 topics/languages/language_names/last_enriched_at
  - `LeaderboardFilter { language: Option<&str>, languages: Option<&[String]>, topics: Option<&[String]>, topic_mode: TopicMode, q: Option<&str> }`
  - `TopicMode::{And, Or}`
  - `top_by_stars/forks/watchers/trending` 接受 `LeaderboardFilter`，SQL 条件：
    - languages: `$langs::text[] IS NULL OR r.language_names && $langs`
    - topics AND: `r.topics @> $topics`
    - topics OR: `r.topics && $topics`
    - q: `full_name ILIKE '%'||q||'%' OR description ILIKE ... OR EXISTS (unnest topics/language_names)`
  - rank: 过滤后 `ROW_NUMBER() OVER (ORDER BY ...)`

- [ ] **Step 1: 扩展 `Board` 与结构体**

在 `models.rs` 增加 `TrackedDaily`、`TopicMode`、`LanguageShare`、扩展 `RepoInput`/`LeaderboardRow`。

- [ ] **Step 2: 写失败测试 — 按 topics AND 过滤**

在 store 测试（或 api 测）中：插入 3 个 repo 不同 topics，查询 `topics=[a,b] AND` 只返回同时含 a 与 b 的行。

- [ ] **Step 3: 实现 `upsert_repo` 与 filtered queries**

更新全部 `top_by_*` / `trending` 签名与 SQL；旧调用点传空 filter 保持行为。

- [ ] **Step 4: 跑测试**

```bash
cd backend && cargo test -p ght-core
# 或
make test
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/crates/core
git commit -m "feat(core): filter leaderboard by q, topics, languages"
```

---

### Task 3: Collector enrichment (topics + languages)

**Files:**
- Create: `backend/crates/collector/src/enrich.rs`
- Modify: `backend/crates/collector/src/search.rs`（Search JSON 增加 `topics: Option<Vec<String>>` 若 API 返回）
- Modify: `backend/crates/collector/src/collect.rs`
- Modify: `backend/crates/collector/src/store.rs` / core upsert 调用
- Test: wiremock 测 languages 解析与 upsert 字段

**Interfaces:**
- Produces: `enrich::normalize_topics(raw: &[String]) -> Vec<String>`
- Produces: `enrich::shares_from_language_map(map: HashMap<String, i64>) -> (Vec<LanguageShare>, Vec<String>)` — pct = bytes/total*100
- Produces: `enrich::fetch_languages(client, token, owner, name) -> Result<...>`
- Collect 流程在落库后或 upsert 时写入 topics/languages；trending-only 仓对空 topics/languages 批补

- [ ] **Step 1: 实现 normalize + shares 纯函数与单测**

```rust
#[test]
fn language_shares_sum_near_100() {
    let mut m = HashMap::new();
    m.insert("Rust".into(), 90);
    m.insert("Python".into(), 10);
    let (shares, names) = shares_from_language_map(m);
    assert_eq!(names, vec!["Rust", "Python"]); // sorted by bytes desc
    assert!((shares[0].pct - 90.0).abs() < 0.01);
}
```

- [ ] **Step 2: wiremock 测 REST languages 端点解析**

Mock `GET /repos/{o}/{n}/languages` → `{"Rust": 900, "Python": 100}`。

- [ ] **Step 3: 接入 collect 主路径**

不阻断公开榜：enrich 失败 log + continue。

- [ ] **Step 4: Commit**

```bash
git add backend/crates/collector backend/crates/core
git commit -m "feat(collector): enrich repos with topics and language shares"
```

---

### Task 4: API — leaderboard response + query params

**Files:**
- Modify: `backend/crates/api/src/routes_leaderboard.rs`
- Test: 同文件 `#[cfg(test)]` 已有 pattern

**Interfaces:**
- Query: `q: Option<String>`, `topics: Option<String>`（逗号分隔）, `topic_mode: Option<String>`（and|or）, `languages: Option<String>`（逗号分隔）
- 兼容旧 `language: Option<String>`（单语言）→ 并入 languages 数组
- `LeaderboardItem` 增加 `topics: Vec<String>`, `languages: Vec<LanguageShareDto>`, `tracked_by_me: bool`（本任务可先恒 false，Task 7 填真）
- 可选响应：`topic_facets` / `language_facets`（v1：基于当前 board/date + q 后、topics/languages 前的 disjunctive 简化版；若时间紧，facets 可仅返回结果集 unnest）

- [ ] **Step 1: 写测试 — trending + topics filter**

Seed 两 repo，请求 `GET /api/leaderboard/trending?topics=ai`，断言 items 长度与 topics 字段存在。

- [ ] **Step 2: 实现参数解析与 to_items 映射**

- [ ] **Step 3: `cargo test -p ght-api`**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add backend/crates/api
git commit -m "feat(api): leaderboard q/topics/languages filters and enriched items"
```

---

### Task 5: Frontend Phase 0 — layout, theme, table density

**Files:**
- Modify: `frontend/src/index.css`
- Modify: `frontend/src/components/Leaderboard.tsx`
- Modify: `frontend/src/components/LeaderboardTable.tsx`
- Modify: `frontend/src/components/Controls.tsx`（密度/描述开关可先做）
- Test: 可选 vitest 纯函数；手工对照原型

**Interfaces:**
- `document.documentElement.dataset.theme = 'light'|'dark'`
- localStorage `ght-theme`
- 页面：`min-width: 960px`，全宽 gutter；去掉 `max-w-5xl` 限制
- Dark：近黑底 + 高对比文字（参考原型 token）
- Table 列：`# | Repo | Lang(主语言暂单列) | 主指标 | 辅指标` — **无 7d 列**
- 描述默认显示；紧凑 1 行 / 舒适 2 行
- 总榜主指标随 metric；辅指标合并，避免双 ★ 表头

- [ ] **Step 1: CSS 变量主题 + Leaderboard 壳全宽**

- [ ] **Step 2: LeaderboardTable 列模型 + 描述默认开**

- [ ] **Step 3: 顶栏 Light/Dark 切换**

- [ ] **Step 4: 本地 `make web` 肉眼对照原型**

- [ ] **Step 5: Commit**

```bash
git add frontend/src
git commit -m "feat(web): full-width leaderboard, theme toggle, denser table UX"
```

---

### Task 6: Frontend — types, search, topics, multi-language UI

**Files:**
- Modify: `frontend/src/types.ts`
- Modify: `frontend/src/api.ts`（若需）
- Modify: `frontend/src/components/Controls.tsx`
- Modify: `frontend/src/components/Leaderboard.tsx`（URL: q, topics, topic_mode, languages）
- Modify: `frontend/src/components/LeaderboardTable.tsx`（topics chips、languages 色条 + 展开）
- Test: vitest 测 URL 解析/构建纯函数（建议抽 `urlState.ts`）

**Interfaces:**
- URL keys: `q`, `topics`（逗号）, `topic_mode`, `languages`（逗号）
- `LanguageShare { name: string; pct: number }`
- `LeaderboardItem.topics: string[]`, `languages: LanguageShare[]`
- 语言 `+N more` 可展开/收起（与原型一致）
- 点击 topic/lang chip → 切换筛选并 reload

- [ ] **Step 1: 扩展 types + 抽 URL 读写并测**

```ts
// urlState.test.ts
expect(parseSearch("?topics=ai,llm&topic_mode=or").topics).toEqual(["ai", "llm"]);
```

- [ ] **Step 2: Controls 关键词 + facets 区（facets 可来自响应或客户端从 items 聚合过渡）**

- [ ] **Step 3: Table 多语言 + topics 展示**

- [ ] **Step 4: `npm test` / `npm run build`**

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add frontend/src
git commit -m "feat(web): keyword, topics, and multi-language leaderboard filters"
```

---

### Task 7: Migration + core — user_tracked_repos

**Files:**
- Create: `backend/migrations/0003_user_tracked_repos.sql`
- Modify: `backend/crates/core/src/store.rs`（track CRUD）
- Modify: `backend/crates/core/src/models.rs`（若需 TrackedRow）

**Interfaces:**
- Table `user_tracked_repos(id, user_id, repo_id, created_at, UNIQUE(user_id, repo_id))` + indexes
- `track_repo(pool, user_id, repo_id) -> Result<(), sqlx::Error>`
- `untrack_repo(pool, user_id, full_name) -> Result<bool, ...>`
- `list_tracked(pool, user_id, filter) -> Vec<TrackedRow>`
- `count_tracked(pool, user_id) -> i64`
- `is_tracked(pool, user_id, repo_id) -> bool`
- `list_all_tracked_full_names(pool) -> Vec<String>` // collector

- [ ] **Step 1: 迁移文件**

```sql
CREATE TABLE user_tracked_repos (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL,
    repo_id     BIGINT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, repo_id)
);
CREATE INDEX idx_user_tracked_user ON user_tracked_repos (user_id);
CREATE INDEX idx_user_tracked_repo ON user_tracked_repos (repo_id);
```

- [ ] **Step 2: store CRUD + 单测（上限 count）**

- [ ] **Step 3: Commit**

```bash
git add backend/migrations/0003_user_tracked_repos.sql backend/crates/core
git commit -m "feat(db): user_tracked_repos for personal repo tracking"
```

---

### Task 8: API — lookup / track / untrack / list

**Files:**
- Create: `backend/crates/api/src/routes_track.rs`
- Modify: `backend/crates/api/src/lib.rs`
- Modify: `backend/crates/api/src/routes_leaderboard.rs`（`tracked_by_me`）
- Modify: `backend/crates/api/src/routes_history.rs`（允许本人跟踪仓）
- Test: wiremock GitHub + 认证 cookie 流程（复用现有 test harness）

**Interfaces:**
- `POST /api/repos/lookup` body `{ full_name?: string, url?: string }` → preview JSON
- `POST /api/repos/track` body 同上 → 201/200 TrackedRepoItem
- `DELETE /api/repos/track?full_name=owner/name` → 204
- `GET /api/repos/tracked` → `{ items: TrackedRepoItem[] }`
- 解析：仅 `github.com/owner/name` 或 `owner/name`；非法 400
- GitHub 404/private → 404；超过 50 → 409
- Track 成功：upsert repo 元数据 + 可选即时 snapshot `tracked_daily` + insert user_tracked
- Status 计算：`on_board` 若当日公开 board 有该 repo；否则有 snapshot 则 `tracking`，否则 `pending`

- [ ] **Step 1: 解析 full_name 纯函数测试**

```rust
assert_eq!(parse_repo_ref("https://github.com/a/b").unwrap(), ("a", "b"));
assert!(parse_repo_ref("https://evil.com/a/b").is_err());
```

- [ ] **Step 2: routes + wiremock 集成测 track 幂等**

- [ ] **Step 3: history 授权：本人 tracked 可读**

- [ ] **Step 4: Commit**

```bash
git add backend/crates/api
git commit -m "feat(api): track/untrack/lookup endpoints for user repos"
```

---

### Task 9: Collector — daily scan of tracked set

**Files:**
- Modify: `backend/crates/collector/src/collect.rs`
- Modify: `backend/crates/collector/src/enrich.rs`（复用）
- Test: wiremock 单仓 snapshot 写入 `tracked_daily`

**Interfaces:**
- After public boards：`for full_name in list_all_tracked_full_names` 批处理拉 repo 信息 + languages + metrics → `upsert_snapshot(..., Board::TrackedDaily, ...)`
- 失败不 fail 整个 collect

- [ ] **Step 1: 实现扫描循环 + 测试**

- [ ] **Step 2: Commit**

```bash
git add backend/crates/collector
git commit -m "feat(collector): snapshot user-tracked repos to tracked_daily"
```

---

### Task 10: Frontend — AddRepoModal + TrackedPanel + board=tracked

**Files:**
- Create: `frontend/src/components/AddRepoModal.tsx`
- Create: `frontend/src/components/TrackedPanel.tsx`
- Modify: `frontend/src/components/Leaderboard.tsx`
- Modify: `frontend/src/components/Controls.tsx`（三视图：趋势/总榜/我的跟踪）
- Modify: `frontend/src/types.ts`、`api.ts`
- Modify: `frontend/src/components/LeaderboardTable.tsx`（已跟踪角标）

**Interfaces:**
- Controls board: `trending | top | tracked`
- AddRepoModal: input → lookup → preview → track
- TrackedPanel: list status、取消跟踪、开历史
- URL `board=tracked`

- [ ] **Step 1: api 封装 track/lookup/list/untrack**

- [ ] **Step 2: AddRepoModal UI 与错误态**

- [ ] **Step 3: TrackedPanel + board 切换**

- [ ] **Step 4: 榜内 `tracked_by_me` 角标**

- [ ] **Step 5: build + 手工走通添加未在榜仓**

- [ ] **Step 6: Commit**

```bash
git add frontend/src
git commit -m "feat(web): add and manage user-tracked repositories"
```

---

### Task 11: sqlx offline cache + README 更新

**Files:**
- Modify: `backend/.sqlx/*` via `make sqlx-prepare`（或项目等价命令）
- Modify: `README.md`（新筛选参数、跟踪功能、主题简述）
- Optional: 链到 design/plan

- [ ] **Step 1: 在有 DB 环境执行 sqlx prepare**

```bash
make sqlx-prepare
# 或
cd backend && cargo sqlx prepare --workspace
```

- [ ] **Step 2: README 补充**

- 列表筛选 query 参数表  
- 用户跟踪说明（不进入公开 top 排名）  
- 主题 localStorage  

- [ ] **Step 3: 全量测试**

```bash
make test
cd frontend && npm test && npm run build
```

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add backend/.sqlx README.md
git commit -m "chore: sqlx prepare and docs for list UX and tracking"
```

---

## Spec Coverage Checklist

| Spec 项 | Task |
|---------|------|
| 全宽 + min-width 960 + sticky + 列模型 + 无 7d sparkline | Task 5 |
| 描述默认 / 密度 / Light-Dark 高对比 | Task 5 |
| topics 存储 + collector | Task 1–3 |
| q / topics AND-OR / 服务端筛选 / rank 重算 | Task 2, 4, 6 |
| 多语言占比 + 展开 + languages OR 筛选 | Task 1–4, 6 |
| 用户跟踪 / pending-tracking-on_board / 上限 | Task 7–10 |
| tracked_daily 不污染公开榜 | Task 7–9 |
| history 支持跟踪仓 | Task 8 |
| URL 可分享筛选 | Task 6, 10 |
| 原型对照 | Tasks 5, 6, 10 |

## Out of scope (explicit)

- AI 打标、人工运营标签后台  
- 列内 sparkline、rank 日变动  
- 虚拟滚动、中文 FTS  
- 私有仓库  
- 把跟踪仓插入公开 top100  

---

## Self-Review Notes

- 无 TBD 占位；facets disjunctive 在 Task 4 允许 v1 简化并写明。  
- `Board::TrackedDaily` 与 API board 字符串全计划一致。  
- 旧 `language` 单参兼容写入 `LeaderboardFilter.languages`。  
- PG 使用本机 local-debug，禁止 docker 起 pgsql。
