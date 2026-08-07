import { useCallback, useEffect, useMemo, useState } from "react";
import AddRepoModal from "./AddRepoModal";
import Controls from "./Controls";
import LeaderboardTable from "./LeaderboardTable";
import RepoHistoryPanel from "./RepoHistoryPanel";
import TrackedPanel from "./TrackedPanel";
import {
  api,
  appendHealthFilterParams,
  listTrackedRepos,
  untrackRepo,
  UnauthorizedError,
} from "../api";
import type {
  LanguageFacet,
  LeaderboardResponse,
  LicenseFacet,
  MetaResponse,
  TopicFacet,
  TrackedRepoItem,
} from "../types";
import {
  applyDensityClasses,
  applyTheme,
  getPreferredTheme,
  readDensity,
  readShowDesc,
  writeDensity,
  writeShowDesc,
  type Density,
  type Theme,
} from "../theme";
import {
  buildSearch,
  hasActiveFilters,
  parseSearch,
  toggleInList,
  type BoardKind,
  type Metric,
  type TopicMode,
  type UrlState,
} from "../urlState";

interface Props {
  username: string;
  isAdmin: boolean;
  onLogout: () => void;
  onOpenAdmin?: () => void;
  onUnauthorized?: () => void;
}

function aggregateFacetsFromItems(
  items: Array<{
    topics?: string[];
    languages?: { name: string }[];
    language?: string | null;
    license?: string | null;
  }>,
): {
  topics: TopicFacet[];
  languages: LanguageFacet[];
  licenses: LicenseFacet[];
} {
  const topicCounts = new Map<string, number>();
  const langCounts = new Map<string, number>();
  const licenseCounts = new Map<string, number>();
  for (const item of items) {
    for (const t of item.topics ?? []) {
      topicCounts.set(t, (topicCounts.get(t) ?? 0) + 1);
    }
    const langs = item.languages ?? [];
    if (langs.length === 0 && item.language) {
      langCounts.set(item.language, (langCounts.get(item.language) ?? 0) + 1);
    } else {
      const seen = new Set<string>();
      for (const share of langs) {
        if (seen.has(share.name)) continue;
        seen.add(share.name);
        langCounts.set(share.name, (langCounts.get(share.name) ?? 0) + 1);
      }
    }
    if (item.license) {
      licenseCounts.set(item.license, (licenseCounts.get(item.license) ?? 0) + 1);
    }
  }
  const topics: TopicFacet[] = [...topicCounts.entries()]
    .map(([topic, count]) => ({ topic, count }))
    .sort((a, b) => b.count - a.count || a.topic.localeCompare(b.topic));
  const languages: LanguageFacet[] = [...langCounts.entries()]
    .map(([language, count]) => ({ language, count }))
    .sort((a, b) => b.count - a.count || a.language.localeCompare(b.language));
  const licenses: LicenseFacet[] = [...licenseCounts.entries()]
    .map(([license, count]) => ({ license, count }))
    .sort((a, b) => b.count - a.count || a.license.localeCompare(b.license));
  return { topics, languages, licenses };
}

export default function Leaderboard({
  username,
  isAdmin,
  onLogout,
  onOpenAdmin,
  onUnauthorized,
}: Props) {
  const initial = parseSearch(window.location.search);
  const [board, setBoard] = useState<BoardKind>(initial.board);
  const [metric, setMetric] = useState<Metric>(initial.metric);
  const [date, setDate] = useState(initial.date);
  const [q, setQ] = useState(initial.q);
  const [topics, setTopics] = useState<string[]>(initial.topics);
  const [topicMode, setTopicMode] = useState<TopicMode>(initial.topicMode);
  const [languages, setLanguages] = useState<string[]>(initial.languages);
  const [licenses, setLicenses] = useState<string[]>(initial.licenses);
  const [excludeArchived, setExcludeArchived] = useState(initial.excludeArchived);
  const [activeWithin, setActiveWithin] = useState<number | null>(
    initial.activeWithin,
  );
  const [dates, setDates] = useState<string[]>([]);
  const [data, setData] = useState<LeaderboardResponse | null>(null);
  const [trackedItems, setTrackedItems] = useState<TrackedRepoItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [historyRepo, setHistoryRepo] = useState<string | null>(null);
  const [theme, setTheme] = useState<Theme>(() => getPreferredTheme());
  const [density, setDensity] = useState<Density>(() => readDensity());
  const [showDesc, setShowDesc] = useState(() => readShowDesc());
  const [addOpen, setAddOpen] = useState(false);
  const [untracking, setUntracking] = useState<string | null>(null);
  const [untrackError, setUntrackError] = useState<string | null>(null);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    applyDensityClasses(density, showDesc);
    writeDensity(density);
    writeShowDesc(showDesc);
  }, [density, showDesc]);

  // Sync filter state → URL (shareable).
  useEffect(() => {
    const state: UrlState = {
      board,
      metric,
      date,
      q,
      topics,
      topicMode,
      languages,
      licenses,
      excludeArchived,
      activeWithin,
    };
    window.history.replaceState(null, "", buildSearch(state));
  }, [
    board,
    metric,
    date,
    q,
    topics,
    topicMode,
    languages,
    licenses,
    excludeArchived,
    activeWithin,
  ]);

  useEffect(() => {
    api<MetaResponse>("/api/meta")
      .then((m) => setDates(m.dates ?? []))
      .catch((e) => {
        if (e instanceof UnauthorizedError) onUnauthorized?.();
      });
  }, [onUnauthorized]);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    setUntrackError(null);
    try {
      if (board === "tracked") {
        const items = await listTrackedRepos({
          q,
          topics,
          languages,
          licenses,
          topicMode,
          excludeArchived,
          activeWithin,
        });
        setTrackedItems(items);
        setData(null);
      } else {
        const params = new URLSearchParams();
        if (date) params.set("date", date);
        if (q.trim()) params.set("q", q.trim());
        if (topics.length) params.set("topics", topics.join(","));
        if (topicMode !== "and") params.set("topic_mode", topicMode);
        if (languages.length) params.set("languages", languages.join(","));
        if (licenses.length) params.set("licenses", licenses.join(","));
        appendHealthFilterParams(params, { excludeArchived, activeWithin });
        const qs = params.toString();
        const path =
          board === "top"
            ? `/api/leaderboard/top?metric=${encodeURIComponent(metric)}${qs ? `&${qs}` : ""}`
            : `/api/leaderboard/trending${qs ? `?${qs}` : ""}`;
        setData(await api<LeaderboardResponse>(path));
        setTrackedItems([]);
      }
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      setError(e instanceof Error ? e.message : "load failed");
    } finally {
      setLoading(false);
    }
  }, [
    board,
    metric,
    date,
    q,
    topics,
    topicMode,
    languages,
    licenses,
    excludeArchived,
    activeWithin,
    onUnauthorized,
  ]);

  useEffect(() => {
    void load();
  }, [load]);

  const { topicFacets, languageFacets, licenseFacets } = useMemo(() => {
    if (board === "tracked") {
      const agg = aggregateFacetsFromItems(trackedItems);
      return {
        topicFacets: agg.topics,
        languageFacets: agg.languages,
        licenseFacets: agg.licenses,
      };
    }
    if (!data) {
      return {
        topicFacets: [] as TopicFacet[],
        languageFacets: [] as LanguageFacet[],
        licenseFacets: [] as LicenseFacet[],
      };
    }
    if (data.topic_facets || data.language_facets || data.license_facets) {
      return {
        topicFacets: data.topic_facets ?? [],
        languageFacets: data.language_facets ?? [],
        licenseFacets: data.license_facets ?? [],
      };
    }
    const agg = aggregateFacetsFromItems(data.items);
    return {
      topicFacets: agg.topics,
      languageFacets: agg.languages,
      licenseFacets: agg.licenses,
    };
  }, [board, data, trackedItems]);

  const onToggleTopic = useCallback((topic: string) => {
    setTopics((cur) => toggleInList(cur, topic.toLowerCase()));
  }, []);

  const onToggleLanguage = useCallback((lang: string) => {
    setLanguages((cur) => toggleInList(cur, lang));
  }, []);

  const onToggleLicense = useCallback((license: string) => {
    setLicenses((cur) => toggleInList(cur, license));
  }, []);

  const onClearFilters = useCallback(() => {
    setQ("");
    setTopics([]);
    setLanguages([]);
    setLicenses([]);
  }, []);

  const onUntrack = useCallback(
    async (fullName: string) => {
      setUntracking(fullName);
      setUntrackError(null);
      try {
        await untrackRepo(fullName);
        setTrackedItems((cur) => cur.filter((r) => r.full_name !== fullName));
        // If currently viewing a public board, refresh so tracked_by_me badge updates.
        if (board !== "tracked") {
          void load();
        }
      } catch (e) {
        if (e instanceof UnauthorizedError) {
          onUnauthorized?.();
          return;
        }
        // Do not use load `error` — that would hide the tracked list.
        setUntrackError(e instanceof Error ? e.message : "取消跟踪失败");
      } finally {
        setUntracking(null);
      }
    },
    [board, load, onUnauthorized],
  );

  const onTracked = useCallback(
    (item: TrackedRepoItem) => {
      setBoard("tracked");
      setTrackedItems((cur) => {
        const without = cur.filter(
          (r) => r.full_name.toLowerCase() !== item.full_name.toLowerCase(),
        );
        return [item, ...without];
      });
      // Reload list after track so status/meta match server (filters applied).
      // Board state update triggers load via effect; force a tick if already on tracked.
      if (board === "tracked") {
        void load();
      }
    },
    [board, load],
  );

  const resultHint = useMemo(() => {
    const n = board === "tracked" ? trackedItems.length : (data?.items.length ?? 0);
    if (board !== "tracked" && !data) return null;
    const boardHint =
      board === "trending"
        ? "趋势榜"
        : board === "tracked"
          ? "我的跟踪"
          : `总榜·${metric}`;
    const langHint = languages.length ? `lang×${languages.length}` : "全部语言";
    const parts = [`${n} 个结果`, boardHint, langHint];
    if (licenses.length) parts.push(`lic×${licenses.length}`);
    if (topics.length) parts.push(`tag ${topicMode.toUpperCase()}`);
    if (q.trim()) parts.push(`q`);
    return parts.join(" · ");
  }, [data, board, metric, languages, licenses, topics, topicMode, q, trackedItems]);

  const showPublicTable = board !== "tracked";

  return (
    <div className="page">
      <header
        className="flex items-center justify-between gap-3"
        style={{ marginBottom: 18 }}
      >
        <h1
          className="m-0 text-[22px] font-semibold tracking-tight"
          style={{ color: "var(--text)" }}
        >
          GH Trending
        </h1>
        <div
          className="flex flex-wrap items-center gap-3.5 text-[13px]"
          style={{ color: "var(--text-3)" }}
        >
          <button
            type="button"
            className="header-btn"
            title="跟踪爬虫未覆盖的仓库"
            onClick={() => setAddOpen(true)}
          >
            ＋ 添加仓库
          </button>
          <div className="theme-toggle" role="group" aria-label="主题">
            <button
              type="button"
              className={theme === "light" ? "active" : undefined}
              onClick={() => setTheme("light")}
              title="浅色主题"
            >
              ☀ Light
            </button>
            <button
              type="button"
              className={theme === "dark" ? "active" : undefined}
              onClick={() => setTheme("dark")}
              title="深色主题"
            >
              ☾ Dark
            </button>
          </div>
          {data?.date && board !== "tracked" && <span>数据截至 {data.date}</span>}
          <span>{username}</span>
          {isAdmin && onOpenAdmin && (
            <button type="button" onClick={onOpenAdmin} className="header-link">
              管理
            </button>
          )}
          <button type="button" onClick={onLogout} className="header-link">
            登出
          </button>
        </div>
      </header>

      <div className="space-y-4">
        <Controls
          board={board}
          metric={metric}
          date={date}
          dates={dates}
          q={q}
          topics={topics}
          topicMode={topicMode}
          languages={languages}
          licenses={licenses}
          excludeArchived={excludeArchived}
          activeWithin={activeWithin}
          topicFacets={topicFacets}
          languageFacets={languageFacets}
          licenseFacets={licenseFacets}
          density={density}
          showDesc={showDesc}
          onBoard={setBoard}
          onMetric={setMetric}
          onDate={setDate}
          onQ={setQ}
          onToggleTopic={onToggleTopic}
          onTopicMode={setTopicMode}
          onToggleLanguage={onToggleLanguage}
          onToggleLicense={onToggleLicense}
          onExcludeArchived={setExcludeArchived}
          onActiveWithin={setActiveWithin}
          onClearFilters={onClearFilters}
          onDensity={setDensity}
          onShowDesc={setShowDesc}
        />

        {(data || board === "tracked") && (
          <div className="result-meta">
            <div className="active-filters">
              {q.trim() && <span className="pill-q">q: {q.trim()}</span>}
              {languages.map((l) => (
                <button
                  key={l}
                  type="button"
                  className="chip selected-only"
                  onClick={() => onToggleLanguage(l)}
                >
                  {l}
                  <span className="x">×</span>
                </button>
              ))}
              {licenses.map((l) => (
                <button
                  key={l}
                  type="button"
                  className="chip selected-only"
                  onClick={() => onToggleLicense(l)}
                >
                  {l}
                  <span className="x">×</span>
                </button>
              ))}
              {topics.map((t) => (
                <button
                  key={t}
                  type="button"
                  className="chip selected-only"
                  onClick={() => onToggleTopic(t)}
                >
                  {t}
                  <span className="x">×</span>
                </button>
              ))}
              {hasActiveFilters({ q, topics, languages, licenses }) && (
                <button
                  type="button"
                  className="clear-filters"
                  onClick={onClearFilters}
                >
                  清除
                </button>
              )}
            </div>
            {resultHint && <div>{resultHint}</div>}
          </div>
        )}

        {board === "tracked" && (
          <TrackedPanel
            items={trackedItems}
            loading={loading}
            error={error}
            untrackError={untrackError}
            hasFilters={hasActiveFilters({ q, topics, languages, licenses })}
            onUntrack={(name) => void onUntrack(name)}
            onSelectRepo={setHistoryRepo}
            onRetry={() => void load()}
            untracking={untracking}
          />
        )}

        {showPublicTable && loading && (
          <div className="space-y-2 py-4" aria-busy="true" aria-label="加载中">
            {[1, 2, 3, 4, 5].map((i) => (
              <div key={i} className="skeleton-row" style={{ opacity: 1 - i * 0.12 }} />
            ))}
          </div>
        )}
        {showPublicTable && error && (
          <div className="space-y-2 py-8 text-center">
            <p style={{ color: "var(--danger)" }}>{error}</p>
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              今日抓取可能未完成，可稍后重试或检查 collector。
            </p>
            <button
              type="button"
              onClick={() => void load()}
              className="text-sm hover:underline"
              style={{ color: "var(--link)" }}
            >
              重试
            </button>
          </div>
        )}
        {showPublicTable && !loading && !error && data && (
          <LeaderboardTable
            items={data.items}
            board={board}
            metric={metric}
            density={density}
            selectedTopics={topics}
            selectedLanguages={languages}
            selectedLicenses={licenses}
            onToggleTopic={onToggleTopic}
            onToggleLanguage={onToggleLanguage}
            onToggleLicense={onToggleLicense}
            onSelectRepo={setHistoryRepo}
          />
        )}
      </div>

      {historyRepo && (
        <RepoHistoryPanel
          fullName={historyRepo}
          board={board}
          metric={metric}
          onClose={() => setHistoryRepo(null)}
          onUnauthorized={onUnauthorized}
        />
      )}

      <AddRepoModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onTracked={onTracked}
        onUnauthorized={onUnauthorized}
      />
    </div>
  );
}
