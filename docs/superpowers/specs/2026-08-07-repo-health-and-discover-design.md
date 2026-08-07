# GH Trending — Repo 健康度信号与库外发现 设计文档

日期：2026-08-07  
状态：**已确认**（用户确认 brainstorming 定稿）  
实施计划：`docs/superpowers/plans/2026-08-07-repo-health-and-discover.md`  
前置设计：

- `docs/superpowers/specs/2026-08-06-github-leaderboard-design.md`
- `docs/superpowers/specs/2026-08-07-leaderboard-list-ux-and-tracking-design.md`

**范围：**  
（1）本地榜单 / 跟踪上的 **健康度信号**（字段、enrich、徽章、筛选）；  
（2）独立 **发现** 视图：经 GitHub Search 库外初筛，再加入跟踪。  

两块数据路径分离；共享同一套 `health` 语义。

---

## 1. 背景与问题

产品目标已从「看 GitHub 日榜」延伸为 **系统开发中的开源选型辅助**。

当前能力：

- 趋势榜 / 总榜 / 我的跟踪；topics / 关键词 / 多语言 / license 筛选
- 精确 `owner/name` lookup → track；90 天历史

选型缺口：

1. **热度 ≠ 可上生产**：缺维护活跃度、是否 archived、issue 规模、发版与仓库年龄等信号。  
2. **候选面被日榜规则限制**：未进 top/trending 的长尾库只能靠已知名字添加，无法按条件在 GitHub 上初筛。  
3. **现有 leaderboard 查询不能直接扩成全站搜索**：数据源、配额、一致性与公开榜排名语义都不同，必须独立功能。

讨论中已排除（本轮）：发现结果 PG 缓存 / TTL（可后续单开）；Compare 工作台；场景 ADR；CVE/依赖图。

---

## 2. 目标与非目标

### 2.1 目标

| # | 目标 |
|---|------|
| G1 | `repos` 持久化选型相关健康字段，并在 enrich / track 路径维护 |
| G2 | 列表以 **健康徽章** 为主扫读；细节（push / issues / release / age）折叠展示 |
| G3 | 榜单与跟踪支持 `exclude_archived`（默认开）、`active_within`（默认关，手动） |
| G4 | API 统一计算 `health`，前端不重复实现 Stale 阈值 |
| G5 | 独立 **发现** tab：代理 GitHub Search 扩大候选，**不**混入 leaderboard 查询 |
| G6 | 发现结果可 **加入跟踪**（现有 track 流水线）；展示是否已跟踪 / 是否已在本地索引 |
| G7 | 发现支持 **混合 Token**：优先用户个人 GitHub PAT，否则回退进程 `GITHUB_TOKEN`；分页 30；Search 一次返回健康字段（列表不做 release 二次请求） |
| G8 | **限流随 token 路径分支**：个人 token → 仅每用户限流；共享 token → 全局 + 每用户（保护共用配额） |

### 2.2 非目标

- 发现搜索结果 **不落库**（不写 `repos` / `snapshots` / 缓存表）；仅用户 track 时写入业务表  
- 不做发现页 query/明细 PG 缓存与 TTL（已讨论，v1 不做；见 §8）  
- 不把跟踪仓或发现结果写入公开 top100  
- 不引入 Search 引擎 / 向量推荐 / AI 摘要  
- 不做 Compare、场景决策记录  
- 不拆 open_issues 中的 PR 计数（沿用 GitHub `open_issues_count` 口径并文档注明）  
- 无私有库  
- v1 不提供用户可配置的 Stale 天数（常量 90）  
- **不用用户 PAT 跑 collector / 全站 enrich**（仅发现 Search 与可选的用户触发的单仓路径；collect 始终用服务端 token）  
- 不实现 GitHub OAuth 装应用；v1 为用户粘贴 **fine-grained / classic PAT**（只读公开库即可）

---

## 3. 产品信息架构

```
┌──────────────────────────────────────────────────────────────────┐
│ Header: 标题 | ＋添加仓库 | Light/Dark | 数据截至 | 用户 | 管理 | 登出 │
├──────────────────────────────────────────────────────────────────┤
│ Board: [趋势榜 | 总榜 | 我的跟踪 | 发现]                             │
│                                                                      │
│ 趋势/总榜/跟踪：既有筛选 + exclude_archived + active_within + 健康徽章 │
│ 发现：独立筛选（dq/dlanguage/…）→ GitHub Search → 加入跟踪            │
└──────────────────────────────────────────────────────────────────┘
```

| 视图 | 数据源 | 写 DB | 说明 |
|------|--------|-------|------|
| 趋势榜 | `trending_daily` ⋈ `repos` | 否（读） | + 健康字段/筛选 |
| 总榜 | `top_*` ⋈ `repos` | 否（读） | 同上 |
| 我的跟踪 | `user_tracked_repos` ⋈ … | 跟踪 CRUD | 同上 |
| **发现** | **GitHub Search API** | **否** | 独立 API；track 才写业务表 |

`board=discover` **仅前端视图状态**，不进入后端 `Board` 枚举，不参与 snapshot board。

---

## 4. 模块 A — 健康度信号（本地数据）

### 4.1 数据模型

Migration 扩展 `repos`：

| 列 | 类型 | 说明 |
|----|------|------|
| `pushed_at` | `TIMESTAMPTZ NULL` | 最近 push |
| `archived` | `BOOLEAN NOT NULL DEFAULT false` | 是否归档 |
| `open_issues_count` | `INT NULL` | GitHub open issues（**含 PR**，官方字段语义） |
| `created_at_gh` | `TIMESTAMPTZ NULL` | 仓库在 GitHub 上的创建时间（避免与 `users.created_at` 混淆） |
| `latest_release_at` | `TIMESTAMPTZ NULL` | 最新 **正式** release 的 `published_at`；无则 null |

索引：

- `CREATE INDEX idx_repos_pushed_at ON repos (pushed_at DESC NULLS LAST);`
- `archived` 过滤可走布尔条件；数据量小，v1 可不建 partial index（可选：`WHERE archived = true`）。

**不落库** `health` / `health_status` 列：阈值变更无需回填。

### 4.2 采集（扩展现有 REST enrich）

1. **`fetch_repo_details`**（`GET /repos/{owner}/{name}`）增加解析：  
   `pushed_at`、`archived`、`open_issues_count`、`created_at` → `created_at_gh`。  
2. **`fetch_latest_release`**（新）：  
   `GET /repos/{owner}/{name}/releases/latest`  
   - 200 → `published_at`  
   - 404 → `null`  
   - 其他错误：log warn；**本次不覆盖**已有 `latest_release_at`（保留上次成功值）。  
3. **调用点**与现网 enrich 对齐：  
   - collector `enrich_today_repos`  
   - track / lookup 即时 meta 路径（与现有 upsert 一致）  
4. **限速**：沿用 `ENRICH_INTERVAL`；details 与 release 之间保留间隔。

Search 日榜抓取路径若已带部分字段，可在 store 时一并写入；**以 enrich / repo details 为权威补全**。

### 4.3 健康徽章（运行时）

常量：`STALE_AFTER_DAYS = 90`。

| `health` | 条件 | UI |
|----------|------|-----|
| `archived` | `archived = true` | 最高优先级，灰/警示 |
| `unknown` | 非 archived，且 `pushed_at` 为 null | 中性 |
| `stale` | 非 archived，且 `now - pushed_at > 90d` | 黄/次级 |
| `active` | 非 archived，且 `pushed_at` 在 90 天内（含恰好 90 天） | 绿 |

**纯函数**（core 共享，单测覆盖边界：恰 90 天、archived 优先、null push）：

```text
health(archived, pushed_at, now) -> active | stale | archived | unknown
```

**已确认规则（实现必须遵守）：**

1. `archived == true` → `archived`  
2. 否则若 `pushed_at == null` → `unknown`（尚未 enrich 或上游未返回）  
3. 否则若 `now - pushed_at > 90 days` → `stale`（恰好等于 90 天仍算 `active`：用 `>` 而非 `>=`）  
4. 否则 → `active`  

发现列表与本地榜使用**同一函数**；`latest_release_at` / `open_issues_count` **不参与** `health` 判定，仅作折叠展示。

### 4.4 折叠细节

徽章默认可见；**hover / focus / 点击** 展开 tooltip 或轻量面板：

| 行 | 内容 |
|----|------|
| Last push | 相对时间 + UTC/本地绝对日期；null → 「未知」 |
| Open issues | 数字；null → 「—」；脚注可说明含 PR |
| Latest release | 相对时间；null → 「无 release / 未知」 |
| Age | 自 `created_at_gh`；null → 「—」 |

三视图（趋势 / 总榜 / 跟踪）同一套徽章与细节。

### 4.5 筛选与 URL（榜单 / 跟踪）

| 参数 | 语义 | 默认 |
|------|------|------|
| `exclude_archived` | `1`/`0`；排除 `archived = true` | **默认 `1`**（API 与前端缺省一致） |
| `active_within` | 正整数天数；保留 `pushed_at >= now - N days` 且非 archived | **缺省不传 = 不过滤** |

- 服务端过滤；过滤后 **rank 重算**（与现有 filter 一致）。  
- `active_within`：`pushed_at IS NULL` 的行 **不命中**。  
- facets 仍基于**当前结果集**计数（v1 既有行为）。

### 4.6 API 响应增量

Leaderboard / tracked 行项目增加：

```json
{
  "pushed_at": "2026-07-01T12:00:00Z",
  "archived": false,
  "open_issues_count": 42,
  "created_at_gh": "2019-03-01T00:00:00Z",
  "latest_release_at": "2026-06-15T00:00:00Z",
  "health": "active"
}
```

时间字段 ISO-8601 / RFC3339 字符串；null 用 JSON `null`。

---

## 5. 模块 B — 发现（库外初筛）

### 5.1 定位

- **扩大筛选范围**：条件搜索 GitHub 公开库，不依赖本地是否已有快照。  
- **与现有查询隔离**：禁止走 `list_leaderboard` / snapshot board SQL。  
- **转化路径**：扫结果 → 健康徽章 → **加入跟踪** → 进入「我的跟踪」与后续 enrich。

### 5.2 入口与 URL 命名空间

- 第四 tab 文案：**发现**。  
- `board=discover` 仅前端；发现专用参数统一 **`d` 前缀**，与榜单参数隔离：

| URL 参数 | API 参数 | 说明 | 默认 |
|----------|----------|------|------|
| `dq` | `q` | 关键词（Search 自由文本） | 空 |
| `dlanguage` | `language` | 单一 `language:` | 无 |
| `dlicense` | `license` | GitHub license 限定符（如 `mit`） | 无 |
| `dmin_stars` | `min_stars` | `stars:>=N` | 无 |
| `dexclude_archived` | `exclude_archived` | `1` → `archived:false` | **`1`** |
| `dactive_within` | `active_within` | 天数 → `pushed:>YYYY-MM-DD`（UTC 日期） | 无 |
| `dsort` | `sort` | `stars` \| `updated` | `stars` |
| `dpage` | `page` | 从 1 | `1` |

**有效查询**：以下至少一项成立，否则前端不请求、提示补充条件：

- `dq` 去空白后非空，或  
- `dlanguage` / `dlicense` / `dmin_stars` / `dactive_within` 任一有效  

切换 board 时：发现参数仅在 `board=discover` 读写；离开发现页可保留在 URL 或剥离——**实现要求：进入非 discover 时不把 `d*` 参数用于榜单请求**；推荐离开时从 URL 移除 `d*` 以免噪音（二选一在 plan 写死；默认 **离开 discover 时 drop `d*`**）。

### 5.3 API

```
GET /api/discover/search
```

- **鉴权**：需登录（与 leaderboard 一致）。  
- **Token 解析**：见 §5.3.0（混合：用户 PAT 优先，否则 `GITHUB_TOKEN`）。  
- **分页**：`per_page` **固定 30**；`page` ∈ `[1, 10]`（最多约 300 条）。  
- **无服务端结果缓存**（v1）：每次有效请求直打 GitHub Search（前端应对相同条件做防抖）。  
- **限流**：见 §5.3.0（在调用 GitHub **之前** 按路径分支检查）。

#### 5.3.0 混合 Token 与限流

**背景**

- GitHub Search 配额绑定在 **认证所用 token** 上（约 30 次/分钟/ token）。  
- 共享 `GITHUB_TOKEN` 时必须有 **全局限流**；用户自带 PAT 时配额已隔离，**只需每用户限流**（仍防刷本站 API）。  
- **单次查询 RTT** 主要由 GitHub 网络决定，换 token / 改限流 **不会** 明显缩短空闲时的单次耗时；混合模式提升的是 **多人并发下的吞吐与更少 429**。

##### Token 选择（每次 discover search）

```text
if 用户已保存个人 GitHub PAT:
    使用用户 PAT          → path = user
else if 进程 GITHUB_TOKEN 非空:
    使用 GITHUB_TOKEN     → path = shared
else:
    503  { "error": "github_token_required" }
    （文案：配置个人 Token 或由管理员配置服务端 GITHUB_TOKEN）
```

响应可带只读元数据（勿回传 token 明文）：

```json
"auth_mode": "user" | "shared"
```

便于前端提示「当前走个人配额 / 共享配额」。

##### 用户 PAT 存储与 API

| 项 | 决议 |
|----|------|
| 存储 | PG 表或 `users` 扩展列：`github_token_ciphertext` + `github_token_set_at`；**加密 at rest**（密钥：`TOKEN_ENCRYPTION_KEY` 或派生自 `JWT_SECRET` 的专用 key，plan 写死一种） |
| 回读 | **永不**在 API 响应中返回完整 PAT；仅 `has_github_token: true/false`、可选 `token_hint`（如末 4 位，可选 v1 不做 hint） |
| 写入 | `PUT /api/me/github-token` body `{ "token": "ghp_…" }`；服务端 trim；可选对 `https://api.github.com/rate_limit` 或 `/user` 做一次校验，失败 400 |
| 删除 | `DELETE /api/me/github-token` |
| 读取状态 | `GET /api/me` 或 `GET /api/me/github-token` → `{ "has_github_token": bool, ... }` |
| 日志 | 禁止打印 token；错误信息脱敏 |
| 范围说明 | UI 提示：公开库只读即可（classic `public_repo` 或 fine-grained Contents/Metadata 只读）；**本站不代替用户访问私有库** |

Collector / 日终 enrich：**只使用** 进程 `GITHUB_TOKEN`，**禁止**读取用户 PAT。

##### 限流矩阵

| 路径 `auth_mode` | 全局限流（共享桶） | 每用户限流 | 占用共享桶？ |
|------------------|-------------------|------------|--------------|
| `user`（个人 PAT） | **不检查** | **是**（默认 **25**/分钟） | **否** |
| `shared`（`GITHUB_TOKEN`） | **是**（默认 **20**/分钟） | **是**（默认 **10**/分钟） | **是** |

环境变量：

| 变量 | 默认 | 含义 |
|------|------|------|
| `DISCOVER_RATE_LIMIT_PER_MIN` | **20** | 仅 **shared** 路径：全站合计即将打 GitHub Search 的次数/分钟 |
| `DISCOVER_RATE_LIMIT_PER_USER_PER_MIN` | **10** | **shared** 路径下每用户/分钟 |
| `DISCOVER_RATE_LIMIT_PER_USER_WITH_TOKEN_PER_MIN` | **25** | **user** 路径下每用户/分钟（贴近个人 Search 配额，略留余量） |

规则：

- 实现：进程内计数器于 `AppState`。  
- **shared**：先全局再每用户；任一超限 → 429，不调 GitHub。  
- **user**：只做每用户桶；**不**增加全局计数。  
- 计数时机：鉴权 + 参数合法 + 已选定 token 且即将请求 GitHub；400/401/503 不占额度。  
- 单实例语义：v1 `replicas: 1`；多副本全局桶不跨 pod（另开变更）。  
- shared 的 20/min 为 collector 预留 Search 余量；user 路径不挤占该余量。

**超限响应**

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 12

{
  "error": "rate_limited",
  "scope": "global",
  "auth_mode": "shared",
  "retry_after_secs": 12
}
```

- `scope`：`global` | `user`  
- `auth_mode`：`user` | `shared`（便于前端文案）

**前端**

- 发现页展示当前 `auth_mode`；无个人 token 时引导「配置 GitHub Token 可提升配额、减少与他人抢共享桶」。  
- 设置入口：Header 或发现页「GitHub Token」：保存 / 清除（不回显密文）。  
- 429：区分全站共享繁忙 vs 个人过于频繁；按 `retry_after_secs` 禁用搜索。  
- 输入防抖保留。

##### 与「缩短查询时间」的关系（产品说明）

| 预期 | 是否成立 |
|------|----------|
| 配置个人 token 后，空闲时单次 Search 更快 | **否**（仍受 GitHub RTT 主导） |
| 多人同时发现时更少卡在全局 429 | **是**（user 路径不占共享桶） |
| 个人可更接近 30 Search/分钟 | **是**（受 `…_WITH_TOKEN_…=25` 与 GitHub 两侧限制） |

#### 5.3.1 拼装 GitHub `q`

将用户条件编译为 Search 限定符，例如：

```text
{user_q} language:Rust license:mit stars:>=100 archived:false pushed:>2026-05-09
```

- `sort`：`stars` 或 `updated`；`order=desc`。  
- 用户 `q` 中若包含危险/冲突片段：v1 **原样拼接**（不实现完整 query AST）；长度上限（如 256）防滥用。

#### 5.3.2 响应

```json
{
  "items": [
    {
      "full_name": "owner/name",
      "html_url": "https://github.com/owner/name",
      "description": "...",
      "language": "Rust",
      "license": "MIT",
      "stars": 1234,
      "forks": 56,
      "topics": ["http"],
      "pushed_at": "2026-07-01T12:00:00Z",
      "archived": false,
      "open_issues_count": 10,
      "created_at_gh": "2019-03-01T00:00:00Z",
      "latest_release_at": null,
      "health": "active",
      "already_tracked": false,
      "in_local_index": false
    }
  ],
  "page": 1,
  "per_page": 30,
  "total_count": 12345,
  "incomplete_results": false,
  "auth_mode": "shared"
}
```

| 字段 | 来源 |
|------|------|
| 列表展示与健康原始字段 | GitHub Search item（`pushed_at`、`created_at`、`archived`、`open_issues_count` 等） |
| `latest_release_at` | 发现列表 **恒为 null**（不二次请求） |
| `health` | 与模块 A **同一函数** |
| `already_tracked` | 当前用户 `user_tracked_repos` |
| `in_local_index` | `repos.full_name` 是否存在（只读标记，**不**作搜索过滤） |
| `auth_mode` | `user` \| `shared`，本次实际使用的 token 路径 |

#### 5.3.3 错误

| 情况 | HTTP |
|------|------|
| 未登录 | 401 |
| 用户无 PAT **且** 无进程 `GITHUB_TOKEN` | 503 `github_token_required` |
| 参数非法 / 空条件 | 400 |
| 保存的 PAT 校验失败（若实现校验） | 400 |
| **应用层限流** | **429** `rate_limited` + `scope` + `auth_mode` + `Retry-After` |
| GitHub 401/403（PAT 失效） | 401/502 包装为可引导「更新 Token」的错误码，如 `github_auth_failed` |
| GitHub 429 | **429** `github_rate_limited` |
| GitHub 5xx / 网络 | 502 |

### 5.4 加入跟踪

- 行操作调用现有 `POST /api/repos/track`（`full_name`）。  
- 已跟踪：按钮禁用或「已跟踪」。  
- 成功后：本地更新该行 `already_tracked=true`；业务侧 upsert `repos` + enrich（含 `latest_release_at`）。  
- **发现 search 本身不 upsert `repos`。**

### 5.5 前端 UI

- Board 增加 **发现**。  
- **独立 Controls**（文案区分「搜索 GitHub」）：关键词、语言、license、min stars、排除归档、仅活跃、sort、分页。  
- **不用** 榜单的 date / metric / topics facets / rank 语义。  
- 结果列：Repo（描述可选）· 语言 · ★ · 健康徽章（折叠细节）· 跟踪；序号可用页内 1..30，非全站 rank。  
- 脚注：`total_count` 来自 GitHub，可能封顶；需 token。  
- 列表请求：**防抖**（如 q 输入 300ms）避免打爆配额。  
- Token 设置：保存 / 清除个人 PAT；展示是否已配置与当前 `auth_mode`（搜索结果返回后）。

### 5.6 后端结构约束

- 新路由模块（如 `routes_discover.rs`）+ 用户 token 路由（可放 `routes` auth/me）。  
- GitHub Search 客户端：可抽共享解析；**不得**改变 collector 日榜 `search_top` 语义。  
- discover handler **禁止**调用 leaderboard 列表查询 / 写入 snapshots。  
- 加密与限流状态放 `AppState`；用户密文只经 store 加解密，不进日志。

---

## 6. 实施顺序

| 顺序 | 内容 | 验收 |
|------|------|------|
| 1 | Migration + models + enrich 写健康字段 | DB 有列；collect/track 后非空 |
| 2 | `health` 纯函数 + leaderboard/tracked 响应与筛选 | 徽章数据 API 可用；默认排除 archived |
| 3 | 前端榜单/跟踪：徽章 + 折叠细节 + 筛选控件 | 扫读与 URL 态 |
| 4 | 用户 PAT 存取 API + 加密列 | 可保存/清除；响应无明文 |
| 5 | `GET /api/discover/search` 混合 token + 分支限流 + 前端发现 tab + track | 双路径限流行为正确；可搜可跟踪 |

---

## 7. 测试要点

**健康度**

- `health()` 边界：archived 优先、恰 90 天、null push → unknown  
- store：`exclude_archived` / `active_within` + rank 重算  
- enrich 解析 fixture：repo meta；releases 200/404  
- API JSON 含新字段  

**发现**

- q 拼装单测（language / license / stars / archived / pushed）  
- page 边界 1..10；空条件 400；无 token 503  
- **限流**：shared 路径全局/用户桶；user 路径不占全局、仅用户桶；未过校验不占额度  
- 混合 token：有 PAT 时 `auth_mode=user` 且 Authorization 使用用户密文解密结果  
- `already_tracked` / `in_local_index` 标记正确  
- PAT API：不回显完整 token；DELETE 后回退 shared  
- 契约：handler 不写 snapshots（可用 spy/集成断言）  
- 前端：url 编解码 `d*`；board=discover 隔离；429 展示与禁用  

---

## 8. 明确推迟：发现结果缓存

Brainstorming 中讨论过：

- PG 缓存查询页 + 明细、TTL 5 天、单仓强制刷新  

**本 spec v1 不做。** 理由：先打通直连 Search + 跟踪转化；配额靠 token、page 上限、前端防抖约束。  

若后续限流成为痛点，另开变更，建议形态：

- `discover_query_cache` + `discover_repo_cache`  
- 与业务 `repos` 分离  
- `already_tracked` 仍现算  

---

## 9. 错误与边界汇总

| 情况 | 行为 |
|------|------|
| 旧行未 enrich | `health=unknown`；`active_within` 排除 null push |
| release 404 | `latest_release_at=null` |
| enrich 限流 | warn + 跳过；下次补齐 |
| 发现：无个人 PAT 且无 `GITHUB_TOKEN` | 503 |
| 发现应用层限流 | 429 `rate_limited` + scope + auth_mode + Retry-After |
| 发现 GitHub 限流 | 429 `github_rate_limited` |
| 用户 PAT 失效 | 引导更新 Token |
| open_issues 含 PR | 文档与 tooltip 注明 |

---

## 10. 验收口径

1. 趋势/总榜/跟踪：可见健康徽章与折叠细节；默认排除 archived；可选手动「90 天活跃」。  
2. 发现：本地库不存在的公开库可被搜到并加入跟踪。  
3. 发现请求不写公开榜、不写 search 缓存表；用户无 PAT 且无服务端 token 时 503。  
4. 混合 token：有 PAT → `auth_mode=user`、不占全局限流；无 PAT → `shared`、全局 20 + 用户 10。  
5. 用户可保存/清除 PAT，API 永不回显完整密钥。  
6. 跟踪后的仓出现在「我的跟踪」，enrich 后具备含 `latest_release_at` 的完整健康字段（在 GitHub 有 release 的前提下）。  
7. 配置个人 token **不作为**「单次查询加速」的验收项（验收吞吐/429 行为即可）。  

---

## 11. 决议记录（brainstorming）

| 议题 | 决议 |
|------|------|
| 健康字段范围 | 标准集 + `created_at_gh`：push / archived / open_issues / latest_release / created |
| 展示 | 徽章为主，字段折叠 |
| 榜单筛选默认 | 默认排除 archived；`active_within` 手动 |
| 采集路径 | 扩展现有 REST enrich + releases/latest |
| 发现入口 | 第四 tab「发现」 |
| 发现落库 | 搜索不落库；仅 track 写业务表 |
| 发现 API 约束 | 混合 token；30/页；page≤10；无任何 token 时 503 |
| 发现缓存 | **v1 不做** |
| Token / 限流 | **混合**：个人 PAT 优先（仅用户 25/min，不占全局）／否则共享 token（全局 20 + 用户 10）；PAT 加密存储；collector 不用用户 PAT |
| 单次延迟 | 换 token **不**承诺降低单次 RTT；改善的是并发与 429 |
