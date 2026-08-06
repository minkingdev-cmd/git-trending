# GH Trending

需登录的 GitHub 每日排行榜：趋势榜（stars today）+ 总榜（star/fork/watch top100，按语言筛选）。

## 架构

| 组件 | 说明 |
|------|------|
| `ght-collector` | 每日抓取 worker（常驻 cron 或 `--once`） |
| `ght-api` | axum 只读 API + 认证（JWT access + refresh 轮换） |
| `ght-admin` | 管理 CLI：bootstrap 用户、邀请码 |
| `frontend` | React + Vite + Tailwind 单页 |

## 快速开始

```bash
cp .env.example .env        # 按需修改（GITHUB_TOKEN 强烈建议填写，watch 榜必需）
set -a && source .env && set +a
make db                     # 启动 PostgreSQL
make admin                  # 创建首个账号（bootstrap）
cd backend && cargo run -p ght-admin -- invite create   # 生成注册用邀请码
make collect                # 首次抓取（需网络；无 token 时 watch 榜自动跳过）
make api                    # 启动 API :8000
make web                    # 前端 dev server :5173
```

打开 http://localhost:5173，用邀请码注册后登录。

## 常用命令

```bash
make db          # 启动 PostgreSQL 并等待就绪
make db-down     # 停止 PostgreSQL
make collect     # 单次抓取
make dev-all     # collector 常驻（每日 COLLECT_TIME 自动跑）
make api         # API :8000
make admin       # bootstrap 用户 admin / change-me-now
make web         # 前端 :5173（代理 /api → :8000）
make test        # 后端 + 前端测试
```

## 环境变量

见 `.env.example`：

| 变量 | 必填 | 说明 |
|------|------|------|
| `DATABASE_URL` | ✅ | PostgreSQL 连接串 |
| `JWT_SECRET` | ✅ | JWT 签名密钥 |
| `GITHUB_TOKEN` | watch 榜必填 | 缺失时 watch 榜跳过 |
| `LANGUAGES` | ❌ | 逗号分隔语言列表 |
| `COLLECT_TIME` | ❌ | 默认 `09:00` |
| `COOKIE_SECURE` | ❌ | 生产设 `true` |

## 数据口径说明

- **趋势榜**：来自 `github.com/trending`，每语言约 25 条，按「stars today」排序。
- **总榜 star/fork**：GitHub Search API 每语言 top 100。
- **总榜 watch**：在 star top 500 候选池内用 GraphQL 查 `watchers.totalCount` 再取 top 100（假设 watch 与 star 强相关）。
- 快照按天落库；UI 展示最新快照。rank 在查询时按语言过滤后用窗口函数重算。

## 测试

```bash
make db
export DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending
cd backend && cargo test
cd frontend && npm test
```
