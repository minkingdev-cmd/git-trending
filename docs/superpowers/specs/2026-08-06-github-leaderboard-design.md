# GH Trending 设计文档

日期：2026-08-06
状态：已确认（brainstorming 定稿）

## 1. 目标

一个需要登录访问的 GitHub 数据排行站点：

- 每天抓取 GitHub 数据，生成三个榜单：
  - **趋势榜**：当日新增 star（来源：github.com/trending 页面官方口径，每语言 ~25 条）
  - **总榜**：按累计 star / fork / watch 数排名，各取 top 100
- 所有榜单可按 repo 主语言筛选
- 点击 repo 名称在新标签页打开对应 GitHub 页面
- 账号自主注册，但必须持有邀请码

**非目标（YAGNI）**：历史趋势图、管理后台、部署方案（先本地跑通，部署后补，但设计上预留容器化接口）。

## 2. 数据口径（关键决策）

| 榜单 | 排名依据 | 条数 | 数据来源 |
|---|---|---|---|
| 趋势榜 | 今日新增 star（stars today） | 每语言 ~25（trending 页面上限） | 爬取 `github.com/trending/{lang}?since=daily` |
| 总榜 star | 累计 star 总数 | 每语言 100 | GitHub Search API `sort=stars` |
| 总榜 fork | 累计 fork 总数 | 每语言 100 | GitHub Search API `sort=forks` |
| 总榜 watch | 累计 subscriber 数 | 每语言 100 | Search API 取 star top500 候选池 + GraphQL 批量查 `watchers.totalCount`，排序取前 100 |

已确认的口径取舍：

- **趋势榜无 fork/watch 口径**：trending 页面不提供这两类增量数据。
- **趋势榜条数不满 100**：官方 trending 每语言只有 ~25 条，接受此上限，不做差分补齐。
- **watch 榜候选池假设**：watch top100 必然落在 star top500 内（watch 与 star 强相关），README 需注明此口径限制。
- **语言覆盖**：全语言 + 可配置热门语言列表（默认 ~20 个：TypeScript、JavaScript、Python、Java、Go、Rust、C、C++、C#、PHP、Ruby、Swift、Kotlin、Shell 等，`LANGUAGES` 环境变量配置）。
- **快照存档**：每日抓取结果按天落库。UI 只展示最新快照，但历史数据保留，未来可加历史趋势功能而无需回补数据。
- **rank 不落库**：同一 repo 会出现在多个语言维度的抓取结果中，排名数字随维度变化；只存数值，查询时过滤语言后用窗口函数重算排名。

## 3. 架构

A+C 混合方案：一个 cargo workspace、三个独立二进制（collector / api / admin）；collector 内置调度器常驻运行，同时支持单次模式。

```
gh-trending/
├── backend/
│   ├── Cargo.toml                     # workspace
│   ├── migrations/                    # SQLx 迁移文件（两个长驻二进制启动时自动执行，幂等）
│   ├── crates/
│   │   ├── core/                      # ght-core lib（三个二进制共享）
│   │   │   └── src/
│   │   │       ├── config.rs          #   环境变量配置（serde + env）
│   │   │       ├── db.rs              #   sqlx::PgPool 工厂
│   │   │       └── models.rs          #   结构体 + 查询（query! 编译期检查）
│   │   ├── collector/                 # ── 二进制 1：抓取 worker ──
│   │   │   └── src/
│   │   │       ├── main.rs            #   clap：默认常驻（每日定时）；--once 跑一次退出
│   │   │       ├── trending.rs        #   trending 页面爬虫（scraper 解析）
│   │   │       ├── search.rs          #   Search API 客户端（reqwest）
│   │   │       └── graphql.rs         #   GraphQL 批量查询（watch 榜）
│   │   ├── api/                       # ── 二进制 2：Web 服务（axum，无状态 JWT 鉴权）──
│   │   │   └── src/
│   │   │       ├── main.rs            #   axum app + 静态文件（tower-http）
│   │   │       ├── routes.rs          #   业务路由 /api/leaderboard/* 等
│   │   │       └── auth/
│   │   │           ├── tokens.rs      #   JWT 签发/校验（jsonwebtoken）、refresh 生成（rand）+ SHA-256
│   │   │           ├── passwords.rs   #   bcrypt crate
│   │   │           ├── refresh.rs     #   refresh_tokens 表 CRUD、轮换、被盗检测（低频路径）
│   │   │           ├── extract.rs     #   RequireAuth extractor：仅验签，零 DB
│   │   │           └── routes.rs      #   /api/auth/*
│   │   └── admin/                     # ── 二进制 3：管理 CLI（clap subcommands）──
├── frontend/                          # React + Vite + Tailwind
├── docker-compose.yml                 # 本地起 PG；collector/api 服务随部署镜像后补
└── Makefile
```

**关键决策**：

- **一个 workspace 多个二进制**：collector 写入与 API 读取共享 ght-core 的模型与查询，schema 变更只改一处。编译产物是三个独立的静态二进制，无运行时依赖。
- **collector 双模式**：默认常驻 + 内置调度（tokio-cron-scheduler，每日 `COLLECT_TIME` 触发）；`--once` 跑一次以退出码报告成败——将来上 k8s CronJob 只改部署参数，代码不动。
- **API 纯只读**：永不调用 GitHub，只查 PG 最新快照。抓取失败不影响 API 服务旧数据，两个故障域隔离。
- **SQLx 编译期查询检查**：`query!` 宏在编译时校验 SQL 与 schema 一致，需要编译环境可连数据库；CI/无库环境用 `SQLX_OFFLINE=true` + 提交到仓库的 `.sqlx/` 查询缓存。

## 4. 数据模型（PostgreSQL）

```sql
CREATE TABLE repos (
    id            BIGSERIAL PRIMARY KEY,
    full_name     VARCHAR(512) NOT NULL UNIQUE,   -- owner/name
    owner         VARCHAR(255) NOT NULL,
    name          VARCHAR(255) NOT NULL,
    html_url      TEXT NOT NULL,
    language      VARCHAR(64),                    -- repo 自身主语言，可为空
    description   TEXT,
    first_seen    DATE NOT NULL
);

CREATE TABLE snapshots (
    id             BIGSERIAL PRIMARY KEY,
    repo_id        BIGINT NOT NULL,        -- 逻辑外键 → repos.id，不加 FK 约束
    snapshot_date  DATE NOT NULL,
    board          VARCHAR(20) NOT NULL,   -- 'trending_daily' | 'top_stars' | 'top_forks' | 'top_watchers'
    stars          INT NOT NULL,           -- 累计 star
    forks          INT NOT NULL,           -- 累计 fork
    watchers       INT,                    -- 累计 subscriber（仅 top_watchers 写入）
    stars_today    INT,                    -- 今日新增 star（仅 trending_daily 写入）
    UNIQUE (repo_id, snapshot_date, board)
);
CREATE INDEX idx_snapshots_query ON snapshots (snapshot_date, board);
CREATE INDEX idx_snapshots_repo ON snapshots (repo_id);

CREATE TABLE users (
    id                BIGSERIAL PRIMARY KEY,
    username          VARCHAR(64) NOT NULL UNIQUE,
    password_hash     TEXT NOT NULL,              -- bcrypt
    created_by_invite BIGINT,                     -- 逻辑外键 → invite_codes.id，仅追溯用
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE invite_codes (
    id          BIGSERIAL PRIMARY KEY,
    code        VARCHAR(32) NOT NULL UNIQUE,      -- rand 随机生成，base64url 编码
    max_uses    INT NOT NULL DEFAULT 1,
    used_count  INT NOT NULL DEFAULT 0,
    revoked     BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- access token 不进表：它是无状态 JWT，请求路径只验签不查库。
-- 本表只为 refresh token 的轮换/吊销/被盗检测服务（低频：仅登录、登出、刷新时访问）。
CREATE TABLE refresh_tokens (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL,                  -- 逻辑外键 → users.id，不加 FK 约束
    token_hash  VARCHAR(64) NOT NULL UNIQUE,      -- refresh token 的 SHA-256 hex（不落明文）
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,                      -- 轮换时置为当前时间；已用 token 再次出现 = 被盗
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_refresh_tokens_user ON refresh_tokens (user_id);
```

**约束策略：不建外键，参照完整性由程序保证。** 所有跨表引用（`snapshots.repo_id`、`users.created_by_invite`、`refresh_tokens.user_id`）只是逻辑外键——建索引加速查询，但不加 `REFERENCES` / `ON DELETE CASCADE`。具体保证方式：

- **插入顺序**：collector 落库时在同一事务内先 upsert `repos`、拿到 `repo_id` 再写 `snapshots`，保证 `repo_id` 必然有效。注册流程同理（先有 user 才有 refresh_token 记录）。
- **级联删除改为程序处理**：没有 FK CASCADE。当前系统不存在删除用户的入口，`refresh_tokens` 由过期清理任务和登出逻辑主动删除；若未来加注销/删号功能，必须在程序里先删该 user 的 refresh_tokens。
- **保留的约束**：仅 `NOT NULL`、`UNIQUE`、主键这类单表内的轻量约束保留（它们不产生跨表检查成本）；跨表的参照完整性全部交给应用层事务。

**落库策略**：全部 upsert。repos 按 `full_name` 冲突更新；snapshots 按 `(repo_id, snapshot_date, board)` 冲突更新。upsert 幂等——同一天重复抓取无副作用，无需分布式锁。

**容量估算**：约 6,500 行快照/天（总榜 3 指标 × ~2000 去重行 + 趋势榜 525），约 350 MB/年，十年 3 GB 量级，无需分区/归档。

## 5. Collector 抓取流程

一次完整运行约 5 分钟：

```
1. 总榜 star/fork（search.rs）
   for lang in [全语言] + LANGUAGES:
       for metric in [stars, forks]:
           GET /search/repositories?q=language:{lang}&sort={metric}&per_page=100
   → 42 请求；认证限速 30 req/min，请求间隔 ~1.5s

2. 总榜 watch（search.rs + graphql.rs）
   for lang in [全语言] + LANGUAGES:
       Search API 按 stars 取 top500（翻 5 页）
       GraphQL 批量查 watchers.totalCount（每请求 ~50 个 repo，alias 打包）
       排序取 top100
   → ~210 GraphQL 请求；需要 GITHUB_TOKEN（缺失时跳过 watch 榜，其余照跑）

3. 趋势榜（trending.rs）
   for lang in [全语言] + LANGUAGES:
       GET https://github.com/trending/{lang}?since=daily，scraper 解析
   → 21 请求；间隔 ~2s，设置 User-Agent

4. 落库
   upsert repos → upsert snapshots（单事务按语言分批提交）

5. 清理
   DELETE FROM refresh_tokens WHERE expires_at < now()
```

**错误处理**：

- 单语言/单请求失败（超时、限流、页面改版解析失败）→ 记日志、跳过，不中断整天任务；全部失败才退出非零码
- Search API 403 rate limit → 指数退避重试 3 次后跳过该语言
- GraphQL 401（无 token）→ 跳过 watch 榜并记 warning
- 无 GITHUB_TOKEN 时 collector 仍可运行，仅降级失去 watch 榜

**调度**：常驻模式用 tokio-cron-scheduler，每日 `COLLECT_TIME`（默认 09:00）执行。抓取各语言循环内可并发请求（GitHub 侧限速仍按上述间隔控制），落库按语言分批提交。

## 6. API 设计（axum）

除 `/api/health` 与 `/api/auth/*` 外，全部路由要求登录（`RequireAuth` extractor，仅 JWT 验签、不查库）。

```
GET  /api/health                    存活检查
GET  /api/meta                      最新快照日期、各榜条数（需登录）
GET  /api/languages                 语言下拉选项：repos 表 distinct + 计数（需登录）
GET  /api/leaderboard/top           总榜 ?metric=stars|forks|watchers&language=&date=（需登录）
GET  /api/leaderboard/trending      趋势榜 ?language=&date=（需登录）
POST /api/auth/register             {username, password, invite_code}
POST /api/auth/login                {username, password}
POST /api/auth/logout
POST /api/auth/refresh              轮换 refresh token + 签发新 access JWT（refresh cookie 自动携带）
GET  /api/auth/me                   登录态探测（200/401），仅验签
```

- `date` 可选，缺省为最新快照日；UI 暂不使用，为历史功能预留。
- rank 用 `ROW_NUMBER() OVER (ORDER BY ...)` 在语言过滤后重算。
- 未登录访问受保护路由返回 401，前端统一切回登录界面。

响应示例（`/api/leaderboard/top`）：

```json
{
  "date": "2026-08-06",
  "board": "top_stars",
  "language": "Python",
  "items": [
    {
      "rank": 1,
      "full_name": "tensorflow/tensorflow",
      "html_url": "https://github.com/tensorflow/tensorflow",
      "description": "An Open Source Machine Learning Framework for Everyone",
      "language": "Python",
      "stars": 190000,
      "forks": 75000
    }
  ]
}
```

## 7. 认证系统（无状态 access + 可轮换 refresh，请求路径零查库）

核心原则：**access token 是无状态 JWT，请求路径只做验签 + 查 exp，不访问数据库**；DB 只在低频路径（登录、登出、每 10 分钟一次的刷新）被访问。两个 token 都放在 httpOnly cookie 里，前端 JS 永远接触不到 token 明文。

| 凭证 | 生命周期 | 存放 | DB 访问频率 |
|---|---|---|---|
| `access_token`（JWT，含 user_id/username/exp） | 15 分钟 | httpOnly cookie `access_token` | **零**（仅验签） |
| `refresh_token`（rand 随机 32 字节，base64url） | 30 天 | httpOnly cookie `refresh_token`（`Path=/api/auth`）+ refresh_tokens 表（存 SHA-256） | 仅登录/登出/刷新 |

**流程**：

1. **登录/注册**：校验密码/邀请码（`used_count < max_uses AND NOT revoked`，注册成功 `used_count + 1`，同一事务）→ 签发 JWT access + 随机 refresh → refresh 的 SHA-256 写入 `refresh_tokens` → Set-Cookie 两个
2. **业务请求鉴权**：`RequireAuth` extractor 读 `access_token` cookie → 用 `JWT_SECRET` 验签 + 校验 exp（**无 DB 查询**）→ 注入 user_id/username；任何失败返回 401
3. **前端定时刷新（主动续期）**：每 10 分钟调 `POST /api/auth/refresh`（refresh cookie 因 `Path=/api/auth` 自动携带）→ 后端轮换 → 重设两个 cookie；页面隐藏（`visibilitychange`）时暂停
4. **轮换 + 被盗检测**：refresh 按 `token_hash` 查行——`used_at` 已置位（旧 token 重放）= 被盗 → 删除该 user 全部 refresh token 并 401；已过期 → 401；有效 → 置 `used_at`、插入新行、返回新 token 对
5. **401 兜底重试**：前端 `api.ts` 拦截 401 → 静默调一次 refresh → 成功则重放原请求，失败则切登录界面（定时刷新之外的第二道保险）
6. **登出**：删除对应 refresh_tokens 行 + 清两个 cookie

**Cookie 属性**：均为 `HttpOnly; SameSite=Lax`，生产环境加 `Secure`；`refresh_token` 额外设 `Path=/api/auth` 缩小发送面。SameSite=Lax 使跨站 POST 不携带 cookie，无需额外 CSRF token。

**首个账号**：CLI 创建（bootstrap，不依赖邀请码）。

## 8. 管理 CLI

```
ght-admin create-user                              # 首个账号（bootstrap，无需邀请码）
ght-admin invite create [--uses N]                 # 生成邀请码（默认 1 次），打印到终端
ght-admin invite list                              # 查看邀请码及使用情况
ght-admin invite revoke <code>                     # 作废邀请码
```

系统不区分用户角色（无管理员权限体系）；`create-user` 只是解决 bootstrap 问题——第一个账号无法通过注册流程（需要邀请码，而邀请码需要账号才能管理）产生。

## 9. 前端（React + Vite + Tailwind）

单页应用，不引入 UI 组件库、不引入 react-router。

**布局**：

```
┌───────────────────────────────────────────────────┐
│  GH Trending        数据截至 2026-08-06   user ▾  │
├───────────────────────────────────────────────────┤
│  [ 趋势榜 ] [ 总榜 ]                               │
│  指标: (●) Star ( ) Fork ( ) Watch   ← 仅总榜      │
│  语言: [全部语言 ▾]                                │
├───────────────────────────────────────────────────┤
│ # │ Repo               │ Lang │ ★     │ Fork │ …  │
└───────────────────────────────────────────────────┘
```

- 未登录时整页替换为登录/注册卡片（同组件表单切换）；`App` 启动先 `GET /api/auth/me` 决定渲染哪个
- 趋势榜多「★ today」列，无 Fork/Watch 指标切换
- Repo 名称：`<a href={html_url} target="_blank" rel="noopener noreferrer">`
- 筛选状态（board/metric/lang）同步到 URL query，刷新不丢、可分享；用原生 `URLSearchParams`
- 数字显示 `Intl.NumberFormat('en', {notation: 'compact'})`（190K）
- 三态处理：loading 骨架、error（含「今日抓取可能未完成」提示）、empty
- `api.ts` 统一 fetch 封装：401 → 静默调一次 refresh 并重放原请求，再失败才切登录界面；10 分钟定时器调 `POST /api/auth/refresh`（页面隐藏时暂停）。前端全程不接触 token（cookie 自动携带）
- 开发：Vite dev server 代理 `/api` → `localhost:8000`
- 生产预留：构建产物 `frontend/dist` 由 axum 通过 tower-http `ServeDir` 挂载，单端口同源部署，无 CORS

## 10. 配置（环境变量）

| 变量 | 必填 | 说明 |
|---|---|---|
| `DATABASE_URL` | ✅ | `postgres://user:pass@host:5432/ghtrending` |
| `JWT_SECRET` | ✅ | JWT 签名密钥 |
| `GITHUB_TOKEN` | watch 榜必填 | 缺失时 watch 榜降级跳过 |
| `LANGUAGES` | ❌ | 逗号分隔，默认内置 ~20 热门语言 |
| `COLLECT_TIME` | ❌ | 常驻模式每日执行时间，默认 `09:00` |

## 11. 测试策略（cargo test）

| 层 | 内容 | 手段 |
|---|---|---|
| 解析器单测（最脆弱一环） | trending HTML → 结构化数据 | 真实页面 fixture（`include_str!`），断言条数与字段 |
| API 响应解析单测 | Search / GraphQL JSON → 模型 | JSON fixture + serde 反序列化测试 |
| API 端点测试 | 榜单过滤、rank 重算、auth 全流程 | axum Router 直接驱动 + 测试库 seed |
| 幂等性测试 | 同日重复抓取结果一致 | fixture 数据 upsert 两遍断言行数 |
| 降级测试 | 无 token 跳 watch 榜、单语言失败不中断 | wiremock 返回 401/403/超时 |
| 安全测试 | 未登录 401、JWT 过期/篡改 401、refresh 轮换、已轮换 token 重放 → 该用户全部 token 被删 | axum Router 直接驱动 |

真实网络请求用 wiremock 全部 mock，不依赖 GitHub 可用性。数据库测试使用独立测试库（`DATABASE_URL` 指向测试 schema，前后清理）。

## 12. 本地运行

```
make db        # docker compose up -d postgres（并等待就绪）
make collect   # cargo run -p ght-collector -- --once   （首次手动）
make api       # cargo run -p ght-api                   （axum，:8000）
make web       # cd frontend && npm run dev             （Vite，代理 /api）
make dev-all   # collector 常驻模式（每日 COLLECT_TIME 自动跑）
make admin     # cargo run -p ght-admin -- create-user  （首个账号 bootstrap）
make test      # cargo test（需测试库）+ npm test
```

本地开发 docker-compose 只起 PostgreSQL（`make db`），collector 与 api 直接 `cargo run`（见上表）；把两个服务编入 compose 需要 Dockerfile 镜像，随部署方案一起后补（见 §14）。迁移文件在两个长驻二进制启动时通过 `sqlx::migrate!()` 自动执行（幂等）。

注意：SQLx `query!` 宏需要编译期可连数据库。本地先 `make db` 再 `cargo build`；无库环境（如 CI 构建镜像）设置 `SQLX_OFFLINE=true` 使用提交到仓库的 `.sqlx/` 查询缓存。

## 13. 技术栈汇总

| 层 | 选型 |
|---|---|
| 语言 | Rust（2021 edition） |
| Web 框架 | axum + tokio + tower-http（静态文件/日志） |
| DB 访问 | SQLx（异步 + 编译期查询检查）+ PostgreSQL，迁移用 sqlx migrate |
| HTTP 客户端 | reqwest（rustls-tls，json feature） |
| HTML 解析 | scraper（CSS 选择器） |
| 调度 | tokio-cron-scheduler（collector 常驻模式） |
| 认证 | jsonwebtoken（access JWT，无状态）+ rand + sha2（refresh token 生成与哈希）+ bcrypt + refresh_tokens 表 |
| CLI | clap（admin 子命令） |
| 序列化/配置 | serde / serde_json / config（环境变量） |
| 日志 | tracing + tracing-subscriber |
| 前端 | React + Vite + Tailwind（无组件库） |
| 测试 | cargo test + wiremock（HTTP mock）+ axum Router 直驱 |
| 依赖管理 | cargo（后端）+ npm（前端） |

## 14. 未来演进（不在本期范围）

- 部署：多阶段 Dockerfile（builder + scratch/distroless 运行镜像，静态二进制）+ k8s 清单（collector 切 CronJob `--once` 模式）
- 历史趋势：数据已按天存档，加查询接口与前端图表即可
- 管理后台：邀请码/用户的 Web 管理界面（当前 CLI 足够）
