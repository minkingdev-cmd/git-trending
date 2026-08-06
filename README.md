# GH Trending

需登录的 GitHub 每日排行榜：趋势榜（stars today）+ 总榜（star/fork/watch top100，按语言筛选）。

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

```bash
cp .env.example .env
set -a && source .env && set +a
make db
make admin                  # admin / change-me-now（bootstrap 管理员）
cd backend && cargo run -p ght-admin -- invite create
make collect                # 首次抓取（建议设置 GITHUB_TOKEN）
make api                    # :8000
make web                    # :5173 代理 /api
```

打开 http://localhost:5173。

## Docker 全栈

```bash
# 可选：先生成 sqlx offline 缓存，便于无 DB 的镜像构建
make db && make sqlx-prepare

export JWT_SECRET=change-me
export GITHUB_TOKEN=ghp_xxx   # 可选
make stack                    # postgres + api 构建启动，并跑一次 collector
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

Collector 使用 `--once`，适合 CronJob；API 为 Deployment。

## 常用命令

```bash
make db / db-down
make collect / dev-all
make api / admin / web
make test
make docker-build / stack / stack-down
make sqlx-prepare
```

## 环境变量

| 变量 | 必填 | 说明 |
|------|------|------|
| `DATABASE_URL` | ✅ | PostgreSQL |
| `JWT_SECRET` | ✅ | JWT 密钥 |
| `GITHUB_TOKEN` | watch 榜 | 缺失则跳过 watch |
| `LANGUAGES` | ❌ | 逗号分隔语言 |
| `COLLECT_TIME` | ❌ | 默认 `09:00` |
| `COOKIE_SECURE` | ❌ | 生产 `true` |
| `STATIC_DIR` | ❌ | 前端 dist 路径（Docker 默认 `/app/frontend/dist`） |

## 功能说明

### 榜单

- 趋势榜 / 总榜（Star · Fork · Watch），按语言筛选
- **历史日期**：下拉选择已有快照日（`?date=YYYY-MM-DD`）
- **Repo 趋势**：点击行内「趋势」查看近 90 天快照折线图

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
- 快照按天落库；rank 查询时重算

## 测试

```bash
make db
export DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
cd backend && cargo test
cd frontend && npm test
```
