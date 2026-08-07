# GH Trending

需登录的 GitHub 每日排行榜：趋势榜（stars today）+ 总榜（star/fork/watch top100）+ 个人跟踪仓；支持关键词 / topics / 多语言筛选，Light/Dark 主题。

设计与计划（本轮列表 UX + 跟踪）：

- [设计文档](docs/superpowers/specs/2026-08-07-leaderboard-list-ux-and-tracking-design.md)
- [实施计划](docs/superpowers/plans/2026-08-07-leaderboard-list-ux-and-tracking.md)
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
| `GITHUB_TOKEN` | watch 榜 | 缺失则跳过 watch |
| `LANGUAGES` | ❌ | 逗号分隔语言 |
| `COLLECT_TIME` | ❌ | 默认 `09:00` |
| `COOKIE_SECURE` | ❌ | 生产 `true` |
| `STATIC_DIR` | ❌ | 前端 dist 路径（Docker 默认 `/app/frontend/dist`） |

## 功能说明

### 榜单

- **趋势榜** / **总榜**（Star · Fork · Watch）/ **我的跟踪**
- 服务端筛选（关键词、topics、多语言 OR）；过滤后 **rank 重算**
- **历史日期**：下拉选择已有快照日（`?date=YYYY-MM-DD`）
- **Repo 趋势**：行内入口打开近 90 天快照折线（列表 **无** 7d sparkline 列）
- 全宽布局（`min-width: 960px`）、sticky 表头、多语言占比条、描述可开关

### 筛选与 URL 状态（可分享）

前端将筛选写入 query string；榜单 API 使用同名参数（服务端过滤）。

| 参数 | 说明 | 示例 |
|------|------|------|
| `board` | `trending` \| `top` \| `tracked`（前端视图） | `board=top` |
| `metric` | 总榜指标：`stars` \| `forks` \| `watchers` | `metric=forks` |
| `date` | 快照日 `YYYY-MM-DD`；缺省为最新 | `date=2026-08-07` |
| `q` | 关键词（full_name / description / topics / 语言名） | `q=llm` |
| `topics` | 逗号分隔 topics（小写）；多选 | `topics=ai,llm` |
| `topic_mode` | topics 组合：`and`（默认）\| `or` | `topic_mode=or` |
| `languages` | 逗号分隔；匹配 `language_names`，**OR** | `languages=Rust,Go` |
| `language` | 兼容旧单参；主语言 equality | `language=Rust` |
| `lang` | 前端兼容：无 `languages` 时当作单语言 | `lang=Python` |

API 示例：

- `GET /api/leaderboard/trending?q=ai&topics=llm&topic_mode=and&languages=Python`
- `GET /api/leaderboard/top?metric=stars&topics=web&topic_mode=or&date=2026-08-07`

响应含 `topic_facets` / `language_facets`（v1：基于当前结果集计数，非完整 disjunctive facets）。

### 用户跟踪

- 登录用户可添加公开仓库（`owner/name` 或 github.com URL），即使 **未进入** 当日公开 top/trending
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
- 快照按天落库；公开榜 rank 在查询时按当前筛选结果重算
- repos 存 `topics`、`languages`（占比 JSON）、`language_names` 供筛选

## 测试

```bash
make db                     # 本地 PG 连通 + 建测试库
export DATABASE_URL=postgres://postgres@localhost:5432/ghtrending
cd backend && cargo test    # 各 crate 测试库隔离（ghtrending_test*）
cd frontend && npm test
```
