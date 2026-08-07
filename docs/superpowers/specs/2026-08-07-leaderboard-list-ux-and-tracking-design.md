# GH Trending — 列表体验、检索与用户跟踪 设计文档

日期：2026-08-07  
状态：**已确认**（brainstorming 定稿，对照可交互原型）  
实施计划：`docs/superpowers/plans/2026-08-07-leaderboard-list-ux-and-tracking.md`  
可交互原型：`docs/prototypes/leaderboard-v2.html`  
前置设计：`docs/superpowers/specs/2026-08-06-github-leaderboard-design.md`

**范围：** 前端列表/筛选/主题；topics 标签与关键词检索；多语言占比；用户添加未索引仓库；配套 API / DB / collector。

---

## 1. 背景与问题

当前产品是需登录的 GitHub 日榜：趋势榜（stars today）+ 总榜（star / fork / watch top100，可按语言筛）。前端为 React + Tailwind 暗色表格式列表。

讨论中确认的核心痛点：

1. **数据密集列表不好扫读**：列语义弱（总榜双 ★）、主指标不突出、描述与密表密度冲突、无稳定视觉节奏。
2. **缺少主题维度检索**：无 tags；仅有单一 `language`；无法按主题/关键词收敛。
3. **语言模型过粗**：只存主语言，无法表达多语言仓库及占比。
4. **趋势入口重复风险**：行内历史按钮与列内 sparkline 职责重叠（已决议只保留弹层入口）。
5. **覆盖面受限**：仓库仅来自爬虫榜单规则；用户无法关注未进 top/trending 的仓库。
6. **观感**：过窄居中栏浪费宽屏；过黑且灰字对比不足；需要 Light / Dark。

---

## 2. 目标与非目标

### 2.1 目标

| # | 目标 |
|---|------|
| G1 | 榜单列表在 **紧凑默认** 下仍可快速扫读主指标与身份信息 |
| G2 | 支持 **GitHub topics 标签** 展示与多选筛选，并支持 **关键词检索** |
| G3 | 每个 repo 支持 **多语言 + 占比**，筛选为「含该语言」多选 |
| G4 | 全宽自适应布局 + **最低宽度**；**Light / Dark** 可切换且对比度合格 |
| G5 | 趋势：**仅** Repo 旁入口打开历史弹层；**不**在列表末列放 sparkline |
| G6 | 登录用户可 **添加并跟踪** 爬虫未索引 / 未进当日榜的公开仓库 |
| G7 | 筛选状态可 **URL 分享**；服务端筛选保证一致性 |

### 2.2 非目标

- 不把用户跟踪仓 **强行写入公开 top100 排名**（避免污染全站榜）。
- 不上重型表格库 / 搜索引擎（规模为日榜百级 + 个人跟踪几十级）。
- v1 不做 AI 自动打标、不做中文分词 FTS、不做虚拟滚动（体量不够）。
- 不做私有仓库跟踪。

---

## 3. 产品信息架构

```
┌──────────────────────────────────────────────────────────────────┐
│ Header: 标题 | ＋添加仓库 | Light/Dark | 数据截至 | 用户 | 管理 | 登出 │
├──────────────────────────────────────────────────────────────────┤
│ Board: [趋势榜 | 总榜 | 我的跟踪]   Metric(仅总榜)   Date           │
│ 关键词 q · 标签 AND/OR · 密度 · 显示介绍 · 清除筛选                 │
│ 语言 facets（多选，OR）                                            │
│ 标签 facets（多选，AND/OR）                                        │
├──────────────────────────────────────────────────────────────────┤
│ 结果计数 · 已选 chips                                              │
├──────────────────────────────────────────────────────────────────┤
│ 总榜/趋势: 表格列表                                                │
│ 我的跟踪: 跟踪管理列表（可取消跟踪 / 看趋势）                        │
└──────────────────────────────────────────────────────────────────┘
         📈 → 历史弹层（90 天）
         ＋添加仓库 → 解析预览 → 加入跟踪
```

### 3.1 三个「榜/视图」

| 视图 | 数据来源 | 排序 | 说明 |
|------|----------|------|------|
| 趋势榜 | `trending_daily` 快照 | stars_today DESC | 公开榜 |
| 总榜 | `top_{stars\|forks\|watchers}` | 对应 metric DESC | 公开榜 |
| 我的跟踪 | `user_tracked_repos` ⋈ repos/snapshots | 默认 stars 或添加时间 | **个人**视图，可含未进公开榜的仓 |

---

## 4. 列表展示方案

### 4.1 布局

- **全宽**：内容区 `width: 100%`，左右 gutter `clamp(16px, 2.5vw, 40px)`，**无**居中 `max-width: 5xl` 限制。
- **最低宽度**：`min-width: 960px`；更窄视口允许横向滚动，保证表格列不崩。
- 表头 **sticky**。

### 4.2 列模型

| 列 | 内容 |
|----|------|
| # | 过滤后重算 rank；等宽数字；Top3 轻微色阶 |
| Repo | avatar（可选，GitHub owner 头像 URL）· `owner`/`name` · 已跟踪角标 · 操作（↗ GitHub、📈 历史）· 可选 description · topics chips |
| Languages | 色条（完整占比）+ 名称/百分比列表（默认可折叠） |
| 主指标 | 趋势：今日 ★（`+N`）；总榜：当前 metric；tabular-nums、字重加强 |
| 辅指标 | 合并展示其余数字（如 `455K ★ · 50K ⑂`），避免总榜双 ★ 表头 |

**已否决**：列表末列 7d sparkline（与 📈 弹层重复，决议 **方案 B**）。

### 4.3 描述（description）

- **默认显示**。
- 舒适密度：最多 2 行；紧凑密度：1 行。
- 「显示介绍」开关可关闭；开关状态建议 `localStorage`。

### 4.4 密度

| 模式 | 行距 | 描述 | 标签 chips | 语言列表默认条数 |
|------|------|------|------------|------------------|
| 紧凑 | 较小 | 1 行（若开启） | 最多 2 +N | 2 + 可展开 |
| 舒适（默认） | 正常 | 2 行（若开启） | 最多 3–4 +N | 3 + 可展开 |

### 4.5 主题

| 主题 | 要求 |
|------|------|
| Dark | 背景更深（近黑）；正文接近白；次级/三级文字避免沉闷中灰，保证对比度 |
| Light | 白/浅灰表面；深绿强调；正文深色 |
| 切换 | 顶栏 Light / Dark；`localStorage` 键 `ght-theme`；无记录时跟随 `prefers-color-scheme` |

### 4.6 趋势交互

- **唯一列表入口**：Repo 旁 📈（或「历史」），打开既有历史弹层（近 90 天）。
- 无列内 sparkline。
- 跟踪列表同样提供「趋势」操作。

---

## 5. 标签（Topics）

### 5.1 来源（分层）

| 层 | 来源 | v1 |
|----|------|----|
| L1 | GitHub topics（Search / REST / GraphQL） | **必做** |
| L2 | 少量高置信规则（如 awesome） | 可选 |
| L3 | 管理员人工标签 | 不做（v2） |

### 5.2 存储

```sql
ALTER TABLE repos
  ADD COLUMN topics TEXT[] NOT NULL DEFAULT '{}';

CREATE INDEX idx_repos_topics_gin ON repos USING GIN (topics);
```

- 入库前：lowercase、trim、去重、排序。
- 每次 collect upsert **覆盖** topics。

### 5.3 展示与筛选

- 行内 chip 最多 N 个，其余 `+N`（标签侧可不做展开，hover title 即可；与语言展开区分）。
- 点击 chip → 加入/取消 topics 筛选。
- 多选默认 **AND**；UI 提供 AND / OR。
- Facet：当前上下文计数；优先 **disjunctive**（计 facet 时忽略 topics 条件本身）；v1 可先结果集内计数。

### 5.4 与 language 分工

- `language` / `languages`：代码语言维度。
- `topics`：主题维度（`machine-learning`、`awesome-list` 等）。
- 不互相替代。

---

## 6. 关键词检索

| 项 | 约定 |
|----|------|
| 参数 | `q` |
| 匹配字段 | `full_name`、`description`、topic 名、语言名 |
| 匹配方式 | 大小写不敏感子串（`ILIKE` 或 `pg_trgm`） |
| 多词 | v1 整串短语；v2 可空格分词 AND |
| 执行位置 | **服务端**（与 URL 一致） |
| 与其它筛选 | 全部 **AND** 叠加 |

---

## 7. 多语言与占比

### 7.1 数据

- 从 GitHub `languages` API 或 GraphQL `languages(first: n) { edges { size node { name color } } }` 取字节占比。
- 存储建议：

```sql
-- JSONB 数组: [{"name":"TypeScript","pct":62.1,"bytes":12345}, ...]
ALTER TABLE repos
  ADD COLUMN languages JSONB NOT NULL DEFAULT '[]';

-- 可选：便于「含某语言」筛选
ALTER TABLE repos
  ADD COLUMN language_names TEXT[] NOT NULL DEFAULT '{}';

CREATE INDEX idx_repos_language_names_gin ON repos USING GIN (language_names);
```

- `language`（主语言）可保留为 `languages[0].name` 的冗余，兼容旧筛选；新 UI 以多语言为准。
- `pct` 按 bytes 归一化到约 100%。

### 7.2 展示

- **色条**：始终展示完整分布（GitHub 风格）。
- **列表**：默认展示 top 2（紧凑）/ top 3（舒适）名称 + 百分比。
- **`+N more`：可点击展开** 全部语言；展开后为「收起」。
- 点击语言名 → 加入/取消语言筛选。
- 无语言数据时显示 `—`。

### 7.3 筛选

- 多选语言，语义：**仓库 languages 中包含任一所选语言（OR）**。
- 与 topics / q / board / date 叠加。

### 7.4 Collector

- 总榜路径：Search 结果后批量补 languages（注意 rate limit）。
- 趋势路径：无 Search 时对缺 languages 的 repo 批量 GraphQL/REST 补全。
- 用户新跟踪仓：添加时即时拉取一次 languages。

---

## 8. 用户添加未索引仓库（跟踪）

### 8.1 产品定义

用户可将 **公开** GitHub 仓库加入「我的跟踪」，即使：

- 从未被爬虫写入，或  
- 未进入当日趋势/总榜 topN。

**公开榜排名规则不变**；跟踪仓进入 **个人视图 + 每日快照目标集**。

### 8.2 用户流程

1. 点击「＋ 添加仓库」。
2. 输入 `owner/name` 或 `https://github.com/owner/name`。
3. 「解析预览」：服务端打 GitHub API，展示 name / description / stars / languages / topics。
4. 状态提示：
   - 已在跟踪 → 不可重复添加；
   - 已在今日公开榜 → 仍可跟踪（角标「已在榜」）；
   - 未在榜 → 加入后「同步中 / pending」，首次快照后变「仅跟踪」。
5. 确认「加入跟踪」→ 跳转或提示「我的跟踪」。
6. 我的跟踪：列表、趋势、取消跟踪。

### 8.3 状态机

```
(未跟踪) --添加成功--> pending --首次快照成功--> tracking
                \                         ^
                 \---- 已在公开榜 ----> on_board (也可视为 tracking 的子状态展示)
tracking / on_board --取消跟踪--> (未跟踪，快照可保留供历史)
```

展示文案：

| 状态 | 文案 |
|------|------|
| on_board | 已在榜 |
| tracking | 仅跟踪 |
| pending | 同步中 |

### 8.4 风控与限制

| 项 | 建议 |
|----|------|
| 仅 public | 私有 / 404 / 禁用返回明确错误 |
| 每用户上限 | 例如 50 |
| 频率 | 例如 10 次/小时/用户 |
| 输入 | 严格解析 owner/name，防 SSRF（只允许 github.com） |

### 8.5 与公开榜关系

- 总榜/趋势 **不** 因用户添加而插入名次。
- 若某跟踪仓次日进入爬虫榜，状态展示为「已在榜」，数据仍一份 `repos` + `snapshots`。
- 公开榜行内若当前用户已跟踪，显示「已跟踪」角标。

---

## 9. 数据模型

### 9.1 现有（摘要）

- `repos`：full_name, owner, name, html_url, language, description, first_seen  
- `snapshots`：(repo_id, snapshot_date, board, stars, forks, watchers, stars_today)

### 9.2 新增 / 变更

```sql
-- 0002_repo_enrichment.sql（示意）
ALTER TABLE repos
  ADD COLUMN topics TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN languages JSONB NOT NULL DEFAULT '[]',
  ADD COLUMN language_names TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN last_enriched_at TIMESTAMPTZ;

CREATE INDEX idx_repos_topics_gin ON repos USING GIN (topics);
CREATE INDEX idx_repos_language_names_gin ON repos USING GIN (language_names);

-- 可选关键词加速
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX idx_repos_full_name_trgm ON repos USING GIN (full_name gin_trgm_ops);
CREATE INDEX idx_repos_description_trgm ON repos USING GIN (description gin_trgm_ops);

-- 0003_user_tracked_repos.sql
CREATE TABLE user_tracked_repos (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL,          -- 逻辑 FK → users.id
    repo_id     BIGINT NOT NULL,          -- 逻辑 FK → repos.id
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, repo_id)
);
CREATE INDEX idx_user_tracked_user ON user_tracked_repos (user_id);
CREATE INDEX idx_user_tracked_repo ON user_tracked_repos (repo_id);
```

**快照 board 扩展（二选一，推荐 A）**：

- **A（推荐）**：跟踪仓与榜单仓共用真实指标写入；对「仅跟踪、不在 top 榜」的仓使用 board = `tracked_daily`（或复用按日单行 `metric` 快照表）。  
- **B**：所有跟踪仓也写入 `top_stars` 等 board —— **不推荐**，会污染榜单查询。

建议 **A**：

```text
board ∈ {
  trending_daily, top_stars, top_forks, top_watchers,
  tracked_daily   -- 用户跟踪仓的日终指标快照（可与公开榜同一 repo 并存）
}
```

「我的跟踪」读数：优先当日 `tracked_daily`，若无则回退任意 board 最新快照或即时 API（添加当下）。

### 9.3 API 模型（响应扩展）

```ts
interface LanguageShare {
  name: string;
  pct: number;      // 0–100
  bytes?: number;
}

interface LeaderboardItem {
  rank: number;
  full_name: string;
  html_url: string;
  description: string | null;
  /** @deprecated 兼容；等于 languages[0]?.name */
  language: string | null;
  languages: LanguageShare[];
  topics: string[];
  stars: number;
  forks: number;
  watchers: number | null;
  stars_today: number | null;
  tracked_by_me?: boolean;
}

interface LeaderboardResponse {
  date: string;
  board: string;
  language: string | null;       // 兼容旧单语言；新客户端用 languages 数组参数
  languages_filter?: string[];
  q?: string | null;
  topics_filter?: string[];
  topic_mode?: "and" | "or";
  items: LeaderboardItem[];
  topic_facets?: { topic: string; count: number }[];
  language_facets?: { language: string; count: number }[];
}

interface TrackedRepoItem {
  full_name: string;
  html_url: string;
  description: string | null;
  languages: LanguageShare[];
  topics: string[];
  stars: number;
  forks: number;
  watchers: number | null;
  status: "on_board" | "tracking" | "pending";
  added_at: string;
}
```

---

## 10. API 设计

### 10.1 公开榜（扩展 query）

```
GET /api/leaderboard/top?metric=stars&date=&q=&topics=a,b&topic_mode=and&languages=Python,Rust
GET /api/leaderboard/trending?date=&q=&topics=&topic_mode=&languages=
```

- `topics`：逗号分隔，lowercase。  
- `topic_mode`：`and` | `or`，默认 `and`。  
- `languages`：逗号分隔；匹配 `language_names && ARRAY[...]`（OR）。  
- 过滤后 **rank 重算**。  
- 响应带 `topics[]`、`languages[]`、`tracked_by_me`、facets。

### 10.2 跟踪

```
POST /api/repos/track
  body: { "full_name": "owner/name" } | { "url": "https://github.com/..." }
  → 201 { item: TrackedRepoItem, preview... }
  → 200 已存在
  → 400 解析失败 / 非 github
  → 404 仓库不存在或非 public
  → 409 超过上限
  → 429 频率限制

DELETE /api/repos/track?full_name=owner/name
  → 204

GET /api/repos/tracked?q=&topics=&languages=
  → { items: TrackedRepoItem[] }

POST /api/repos/lookup   # 可选：仅预览不写入
  body: { full_name | url }
  → { full_name, description, languages, topics, stars, forks, on_leaderboard, already_tracked }
```

均需登录（现有 JWT）。

### 10.3 历史

- 现有 `/api/repo/history` 扩展：允许 **自己跟踪的 repo**（即使不在公开榜 board）按 `tracked_daily` 或可用 board 查点。

---

## 11. Collector 改造

### 11.1 现有流程（不变骨架）

1. 总榜 star/fork/watch  
2. 趋势 HTML  
3. upsert repos + snapshots  

### 11.2 增量

1. **写入 topics / languages**（Search 字段 + 批补 GraphQL）。  
2. **跟踪集扫描**：

```text
SELECT DISTINCT r.full_name
FROM user_tracked_repos t
JOIN repos r ON r.id = t.repo_id
```

对每个（或批处理）拉 REST/GraphQL metrics → upsert `tracked_daily` 快照；刷新 description/topics/languages。  
3. Rate limit：与 watchers 批处理共用 token 预算；失败记日志，不阻断公开榜。  
4. 用户 POST track 时：**同步** 拉一次 GitHub 并写首包 snapshot，状态尽快离开 pending。

---

## 12. 前端方案

### 12.1 技术栈

保持 React + Vite + Tailwind，不引入 Ant Design Table。

### 12.2 主要改动面

| 模块 | 改动 |
|------|------|
| `Leaderboard.tsx` | URL 同步 q/topics/languages/topic_mode；加载；主题；添加仓库入口 |
| `Controls.tsx` | 三视图、搜索、facets、密度/描述、AND/OR |
| `LeaderboardTable.tsx` | 新列模型、语言展开、topics、角标、无 7d 列 |
| `AddRepoModal.tsx`（新） | 解析预览 / 提交跟踪 |
| `TrackedPanel.tsx`（新） | 我的跟踪列表 |
| `RepoHistoryPanel.tsx` | 支持跟踪仓 history |
| `types.ts` / `api.ts` | 新类型与端点 |
| 主题 | CSS 变量 + `data-theme` + localStorage |

### 12.3 URL 约定

```
?board=trending|top|tracked
&metric=stars|forks|watchers   # board=top
&date=YYYY-MM-DD
&q=
&topics=awesome-list,ai
&topic_mode=and|or
&languages=Python,TypeScript
```

### 12.4 原型

可交互静态原型：`docs/prototypes/leaderboard-v2.html`  
已覆盖：全宽/min-width、Light/Dark、描述默认开、多语言展开、标签/关键词、跟踪添加、无 7d 列。

---

## 13. 筛选语义总表

| 维度 | 算子 | 默认 |
|------|------|------|
| board / metric / date | 精确 | 最新 date |
| languages | 多选 OR（含该语言） | 空 = 全部 |
| topics | 多选 AND 或 OR | AND |
| q | 子串 OR 字段 | 空 |
| 组合 | 维度间 AND | — |
| rank | 结果集内重算 | — |

---

## 14. 分阶段交付

### Phase 0 — 列表体验与主题（前端为主）

- 全宽 + min-width 960  
- 列模型、主/辅指标、sticky 表头  
- 描述默认开 + 密度  
- Light/Dark + 对比度  
- 去掉任何列内 sparkline；保留 📈  
- **验收**：宽屏铺满；Dark 字清晰；总榜无双 ★ 歧义  

### Phase 1 — Topics + 关键词

- migration topics  
- collector 写 topics  
- API q / topics / topic_mode  
- Controls + 行内 chip  
- **验收**：URL 可复现；AND/OR 正确；过滤后 rank 连续  

### Phase 2 — 多语言占比

- migration languages / language_names  
- collector 补 languages  
- 行内色条 + 可展开列表  
- languages 多选筛选 + facets  
- **验收**：多语言仓占比可见；+N 可展开；筛选 OR  

### Phase 3 — 用户跟踪

- `user_tracked_repos` + `tracked_daily`  
- lookup / track / untrack / list API  
- AddRepo 弹层 + 我的跟踪视图  
- collector 扫跟踪集  
- history 对跟踪仓可用  
- **验收**：未在榜仓可添加；pending→tracking；取消跟踪；公开榜不因添加改变名次  

### Phase 4 — 增强（可选）

- avatar、rank 日变动、batch spark 仅弹层增强  
- disjunctive facets  
- 跟踪上限管理后台  
- topics blocklist  

---

## 15. 验收标准（汇总）

| # | 场景 | 期望 |
|---|------|------|
| 1 | 打开总榜 | 主指标清晰，描述默认可见，全宽 |
| 2 | 切换 Light/Dark | 持久化；Dark 对比度可接受 |
| 3 | topics 多选 AND | 仅同时含所有 tag 的行 |
| 4 | q=roadmap | 名称/描述/tag 命中 |
| 5 | languages=Rust,Go | 含其一即可 |
| 6 | 语言 +N more | 点击展开/收起 |
| 7 | 📈 | 打开历史；无 7d 列 |
| 8 | 添加未在榜仓 | 进入我的跟踪；产生快照路径 |
| 9 | 添加已在榜仓 | 可跟踪，角标已在榜 |
| 10 | 重复添加 | 幂等提示 |
| 11 | 取消跟踪 | 从我的跟踪消失；公开榜不受影响 |
| 12 | 分享 URL | 他人登录后筛选一致（跟踪视图除外，仅本人） |

---

## 16. 风险与缓解

| 风险 | 缓解 |
|------|------|
| GitHub API 配额 | 批处理、只补缺失、缓存 languages/topics、token 必配 |
| topics/languages 噪声 | facet top N；语言展示折叠；可选 blocklist |
| 跟踪集膨胀拖慢 collect | 每用户上限；全局跟踪去重后批处理；超时跳过 |
| 用户期望「添加后上总榜」 | 文案明确：仅个人跟踪，不改变公开排名 |
| URL 过长 | topics/languages 数量 UI 限制 |

---

## 17. 已拍板决策（讨论结论）

| 决策 | 结论 |
|------|------|
| 描述 | 默认显示，可关 |
| 密度 | 默认舒适；可切紧凑 |
| 布局 | 全宽 + min-width 960 |
| 主题 | Light / Dark，Dark 更深、文字更高对比 |
| 趋势 | **方案 B**：只保留 📈 弹层，去掉列 sparkline |
| Topics 多选 | 默认 AND |
| 关键词 | name + description + topic（+ 语言名） |
| 筛选位置 | 服务端 + URL |
| 语言 | 多语言 + 占比；筛选 OR；+N 可展开 |
| 未索引仓 | 用户跟踪模型，不污染公开榜 |
| 公开榜与跟踪 | 分视图；数据层共享 repos，快照 board 分离 |

---

## 18. 文档与原型索引

| 资源 | 路径 |
|------|------|
| 本设计（spec） | `docs/superpowers/specs/2026-08-07-leaderboard-list-ux-and-tracking-design.md` |
| 实施计划（plan） | `docs/superpowers/plans/2026-08-07-leaderboard-list-ux-and-tracking.md` |
| 可交互原型 | `docs/prototypes/leaderboard-v2.html` |
| 原系统设计 | `docs/superpowers/specs/2026-08-06-github-leaderboard-design.md` |

---

## 19. 建议下一步

1. 按 superpowers **实施计划**逐 Task 落地（推荐 subagent-driven-development）。  
2. 实现时以原型 `leaderboard-v2.html` 为 UI 对照，API/DB 以本文第 9–11 节与 plan Interfaces 为准。  
3. 若需拆分并行：可按 plan 中 Task 1–4（后端 enrichment+API）、Task 5–6（前端列表/检索）、Task 7–10（跟踪）三条线，但须先合入 migration。
