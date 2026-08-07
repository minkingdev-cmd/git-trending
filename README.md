# GH Trending

需登录的 GitHub 每日排行榜：趋势榜（stars today）+ 总榜（star/fork/watch top100）+ 个人跟踪仓 + **发现**（库外 GitHub Search）；支持关键词 / topics / 多语言 / **健康度**筛选，Light/Dark 主题。

设计与计划：

- [健康度 + 发现 设计](docs/superpowers/specs/2026-08-07-repo-health-and-discover-design.md) · [实施计划](docs/superpowers/plans/2026-08-07-repo-health-and-discover.md)
- [列表 UX + 跟踪 设计](docs/superpowers/specs/2026-08-07-leaderboard-list-ux-and-tracking-design.md) · [实施计划](docs/superpowers/plans/2026-08-07-leaderboard-list-ux-and-tracking.md)
- [可交互原型](docs/prototypes/leaderboard-v2.html)

## 架构

| 组件 | 说明 |
|------|------|
| `ght-collector` | 每日抓取 worker（常驻 cron 或 `--once`） |
| `ght-api` | axum API + 认证 + 静态前端 |
| `ght-admin` | 管理 CLI：bootstrap 用户、邀请码 |
| `frontend` | React + Vite + Tailwind 单页 |
| `deploy/k8s` | Deployment + CronJob 示例 |

**管理员**：通过 CLI 创建的 bootstrap 用户（无邀请码）自动拥有管理权限，可在前端「管理」页维护邀请码与用户列表。

## 快速开始（本地开发）

前置：本机部署的 PostgreSQL（local-debug 栈，`localhost:5432`，trust 认证）。
**不使用 docker PG 实例。**

```bash
cp .env.example .env
set -a && source .env && set +a
make db                     # 检查本地 PG 连通并幂等建库（ghtrending*）
make admin                  # admin / change-me-now（bootstrap 管理员）
cd backend && cargo run -p ght-admin -- invite create
make collect                # 首次抓取（建议设置 GITHUB_TOKEN）
make api                    # :8000
make web                    # :5173 代理 /api
```

打开 http://localhost:5173。

## Docker 应用栈（PG 仍用本机实例）

容器经 `host.docker.internal:5432` 连接宿主本地 PG；若宿主 pg_hba 对
docker 网段要求口令，需为 postgres 用户配置密码。

```bash
# 可选：先生成 sqlx offline 缓存，便于无 DB 的镜像构建
make db && make sqlx-prepare

export JWT_SECRET=change-me
export GITHUB_TOKEN=ghp_xxx   # 可选
make stack                    # api 构建启动，并跑一次 collector
```

访问 http://localhost:8000（API 同源托管前端）。

```bash
make stack-down
```

## Kubernetes

示例清单在 `deploy/k8s/`：

1. 构建并推送镜像 `gh-trending:latest`
2. 复制 `secret.example.yaml` → 填真实密钥 → apply
3. `kubectl apply -f deploy/k8s/api-deployment.yaml`
4. `kubectl apply -f deploy/k8s/collector-cronjob.yaml`
5. （可选）`ingress.example.yaml` 配置域名与 TLS

探针：`/api/health`（存活）、`/api/ready`（DB 就绪）。

## 常用命令

```bash
make db / db-down
make collect / dev-all
make api / admin / web
make test
make docker-build / stack / stack-daemon / stack-down
make sqlx-prepare
```

## CI

GitHub Actions（`.github/workflows/ci.yml`）在 push/PR 时运行：

- backend：Postgres service + `cargo test --workspace`（`SQLX_OFFLINE=true`）
- frontend：`npm test` + `npm run build`

## 环境变量

| 变量 | 必填 | 说明 |
|------|------|------|
| `DATABASE_URL` | ✅ | PostgreSQL |
| `JWT_SECRET` | ✅ | JWT 密钥 |
| `TOKEN_ENCRYPTION_KEY` | ❌ | 用户 GitHub PAT 加密密钥：64 位 hex 或 32 字节 base64；缺省则 `SHA-256(JWT_SECRET \|\| "ght-github-token-v1")` |
| `GITHUB_TOKEN` | 推荐 | 进程共享 token：collector / enrich / watch 榜；**发现**在用户未配置个人 PAT 时回退使用。缺失则跳过 watch；发现无用户 PAT 且无本变量 → 503 |
| `DISCOVER_RATE_LIMIT_PER_MIN` | ❌ | 发现 **shared** 路径全站合计上限/分钟，默认 `20` |
| `DISCOVER_RATE_LIMIT_PER_USER_PER_MIN` | ❌ | 发现 **shared** 路径每用户上限/分钟，默认 `10` |
| `DISCOVER_RATE_LIMIT_PER_USER_WITH_TOKEN_PER_MIN` | ❌ | 发现 **user**（个人 PAT）路径每用户上限/分钟，默认 `25` |
| `LANGUAGES` | ❌ | 逗号分隔语言 |
| `COLLECT_TIME` | ❌ | 默认 `09:00` |
| `COOKIE_SECURE` | ❌ | 生产 `true` |
| `STATIC_DIR` | ❌ | 前端 dist 路径（Docker 默认 `/app/frontend/dist`） |

示例（见 `.env.example`）：

```bash
DISCOVER_RATE_LIMIT_PER_MIN=20
DISCOVER_RATE_LIMIT_PER_USER_PER_MIN=10
DISCOVER_RATE_LIMIT_PER_USER_WITH_TOKEN_PER_MIN=25
# TOKEN_ENCRYPTION_KEY optional; else derived from JWT_SECRET
GITHUB_TOKEN=   # shared fallback for discover + collector
```

## 功能说明

### 榜单

- **趋势榜** / **总榜**（Star · Fork · Watch）/ **我的跟踪** / **发现**
- 服务端筛选（关键词、topics、多语言 OR、健康度）；过滤后 **rank 重算**
- **历史日期**：下拉选择已有快照日（`?date=YYYY-MM-DD`）
- **Repo 趋势**：行内入口打开近 90 天快照折线（列表 **无** 7d sparkline 列）
- 全宽布局（`min-width: 960px`）、sticky 表头、多语言占比条、描述可开关
- **健康徽章**：`active` / `stale`（push &gt; 90 天）/ `archived` / `unknown`（尚未 enrich）；hover/点击展开 last push、open issues（含 PR）、latest release、仓库年龄

### 筛选与 URL 状态（可分享）

前端将筛选写入 query string；榜单 API 使用同名参数（服务端过滤）。

| 参数 | 说明 | 示例 |
|------|------|------|
| `board` | `trending` \| `top` \| `tracked` \| `discover`（前端视图；`discover` **不**进入后端 Board） | `board=top` |
| `metric` | 总榜指标：`stars` \| `forks` \| `watchers` | `metric=forks` |
| `date` | 快照日 `YYYY-MM-DD`；缺省为最新 | `date=2026-08-07` |
| `q` | 关键词（full_name / description / topics / 语言名） | `q=llm` |
| `topics` | 逗号分隔 topics（小写）；多选 | `topics=ai,llm` |
| `topic_mode` | topics 组合：`and`（默认）\| `or` | `topic_mode=or` |
| `languages` | 逗号分隔；匹配 `language_names`，**OR** | `languages=Rust,Go` |
| `language` | 兼容旧单参；主语言 equality | `language=Rust` |
| `lang` | 前端兼容：无 `languages` 时当作单语言 | `lang=Python` |
| `exclude_archived` | `1`/`0`；排除归档仓 | 默认 **`1`**；`exclude_archived=0` 关闭 |
| `active_within` | 正整数天数；仅保留 `pushed_at` 在 N 天内且非归档 | 缺省不传 = 不过滤；`active_within=90` |

API 示例：

- `GET /api/leaderboard/trending?q=ai&topics=llm&topic_mode=and&languages=Python`
- `GET /api/leaderboard/top?metric=stars&topics=web&topic_mode=or&date=2026-08-07`
- `GET /api/leaderboard/trending?exclude_archived=1&active_within=90`

响应含 `topic_facets` / `language_facets`（v1：基于当前结果集计数，非完整 disjunctive facets）；行项目含 `health`、`pushed_at`、`archived`、`open_issues_count`、`created_at_gh`、`latest_release_at`。

### 发现（库外初筛）

独立 tab，**不**走 leaderboard / snapshot SQL；每次有效请求直打 GitHub Search（无服务端结果缓存）。登录后可用。

| URL 参数 | API 参数 | 说明 | 默认 |
|----------|----------|------|------|
| `dq` | `q` | 关键词 | 空 |
| `dlanguage` | `language` | 单一语言 | 无 |
| `dlicense` | `license` | license 限定符（如 `mit`） | 无 |
| `dmin_stars` | `min_stars` | `stars:>=N` | 无 |
| `dexclude_archived` | `exclude_archived` | `1` → Search `archived:false` | **`1`** |
| `dactive_within` | `active_within` | 天数 → `pushed:>YYYY-MM-DD` | 无 |
| `dsort` | `sort` | `stars` \| `updated` | `stars` |
| `dpage` | `page` | 页码 1..=10；`per_page` 固定 30 | `1` |

至少一项有效条件（非空 `dq` 或 language / license / min_stars / active_within）才可搜索。

- API：`GET /api/discover/search`
- 响应含 `auth_mode`（`user` \| `shared`）、`already_tracked`、`in_local_index`；`latest_release_at` 在发现列表 **恒为 null**（不做二次 release 请求）
- **加入跟踪**：沿用 `POST /api/repos/track`；search 本身不写 `repos` / snapshots

#### 混合 Token

| 优先级 | 来源 | `auth_mode` |
|--------|------|-------------|
| 1 | 用户已保存的个人 GitHub PAT（加密存库） | `user` |
| 2 | 进程 `GITHUB_TOKEN` | `shared` |
| 否则 | — | **503** `github_token_required` |

- 个人 PAT：`PUT /api/me/github-token` 保存、`DELETE /api/me/github-token` 清除；API **永不**回传完整 token，仅 `has_github_token`
- **Collector / 全站 enrich 只用进程 `GITHUB_TOKEN`**，不读取用户 PAT

#### 发现限流（应用层，调用 GitHub 前）

| 路径 | 全局限流 | 每用户限流 | 默认 |
|------|----------|------------|------|
| `user`（个人 PAT） | 不检查 | 是 | 25/分钟 |
| `shared`（`GITHUB_TOKEN`） | 是 | 是 | 全局 20 + 用户 10 /分钟 |

超限：**429** `rate_limited`，body 含 `scope`（`global` \| `user`）、`auth_mode`、`retry_after_secs`，并带 `Retry-After`。GitHub 侧 429 包装为 `github_rate_limited`。

### 用户跟踪

- 登录用户可添加公开仓库（`owner/name` 或 github.com URL），即使 **未进入** 当日公开 top/trending；也可从 **发现** 结果一键跟踪
- API：`POST /api/repos/lookup` 预览 → `POST /api/repos/track` 跟踪 → `DELETE /api/repos/track` 取消；列表 `GET /api/repos/tracked`
- 状态：`pending`（同步中）→ `tracking`（仅跟踪）/ `on_board`（当日亦在公开榜）
- **每用户上限 50**；超限返回 409
- 跟踪仓快照 board = **`tracked_daily`**，与公开榜隔离
- **不会**把跟踪仓插入公开 top100 名次；公开榜 rank 不受个人 track 影响
- 历史接口支持本人跟踪仓（`tracked_daily`）；collector 日终扫描跟踪集并写快照

### 主题与展示偏好（localStorage）

| 键 | 值 | 说明 |
|----|-----|------|
| `ght-theme` | `light` \| `dark` | 主题；无记录时跟随 `prefers-color-scheme` |
| `ght-density` | `compact` \| `comfortable` | 行密度（默认 comfortable） |
| `ght-show-desc` | `1` \| `0` | 是否显示仓库描述（默认显示） |

通过 `document.documentElement.dataset.theme` 与 body 密度 class 生效。

### 认证

- 邀请码注册；access JWT 15 分钟 + refresh 30 天轮换
- 前端 10 分钟静默 refresh；401 自动重试一次

### 管理后台

- 仅 bootstrap 管理员可见
- 生成 / 作废邀请码、查看用户列表
- 亦可用 CLI：`ght-admin invite create|list|revoke`

## 数据口径

- **趋势榜**：`github.com/trending`，每语言约 25 条
- **总榜 star/fork**：Search API top 100
- **总榜 watch**：star top500 候选池 + GraphQL `watchers.totalCount` 再取 top100
- **跟踪仓**：REST/GraphQL 即时 + collector 日终 → `tracked_daily`（**不**进入公开 top 排名）
- **发现**：GitHub Search 实时；不写 DB；track 后才进入业务表与 enrich（含 `latest_release_at`）
- 快照按天落库；公开榜 rank 在查询时按当前筛选结果重算
- repos 存 `topics`、`languages`（占比 JSON）、`language_names` 及健康字段（`pushed_at` / `archived` / `open_issues_count` / `created_at_gh` / `latest_release_at`）供筛选与徽章
- `health` **不落库**，API 运行时按 `archived` + `pushed_at` 与 90 天阈值计算

## 本地冒烟清单（健康度 + 发现）

1. 榜单行显示健康徽章；默认排除 archived（URL 无 `exclude_archived=0`）
2. 未配置个人 PAT 且无进程 `GITHUB_TOKEN` 时，发现搜索返回 **503** `github_token_required`
3. 配置个人 PAT 后搜索，响应 `auth_mode=user`
4. 从发现结果「加入跟踪」后，在 **我的跟踪** 可见

## 测试

```bash
make db                     # 本地 PG 连通 + 建测试库
export RUSTUP_TOOLCHAIN=stable
export DATABASE_URL=postgres://postgres@localhost:5432/ghtrending
cd backend && SQLX_OFFLINE=true cargo test --workspace
cd frontend && npm test && npm run build
# 或：make test（backend cargo test + frontend npm test；不含 build）
```
