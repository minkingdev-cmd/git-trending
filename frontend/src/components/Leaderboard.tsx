import { useCallback, useEffect, useState } from "react";
import Controls, { type BoardKind, type Metric } from "./Controls";
import LeaderboardTable from "./LeaderboardTable";
import RepoHistoryPanel from "./RepoHistoryPanel";
import { api, UnauthorizedError } from "../api";
import type { LanguageOption, LeaderboardResponse, MetaResponse } from "../types";
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

interface Props {
  username: string;
  isAdmin: boolean;
  onLogout: () => void;
  onOpenAdmin?: () => void;
  onUnauthorized?: () => void;
}

function readUrl(): { board: BoardKind; metric: Metric; lang: string; date: string } {
  const params = new URLSearchParams(window.location.search);
  const board = params.get("board") === "top" ? "top" : "trending";
  const metricRaw = params.get("metric");
  const metric: Metric =
    metricRaw === "forks" || metricRaw === "watchers" ? metricRaw : "stars";
  return {
    board,
    metric,
    lang: params.get("lang") ?? "",
    date: params.get("date") ?? "",
  };
}

export default function Leaderboard({
  username,
  isAdmin,
  onLogout,
  onOpenAdmin,
  onUnauthorized,
}: Props) {
  const initial = readUrl();
  const [board, setBoard] = useState<BoardKind>(initial.board);
  const [metric, setMetric] = useState<Metric>(initial.metric);
  const [lang, setLang] = useState(initial.lang);
  const [date, setDate] = useState(initial.date);
  const [dates, setDates] = useState<string[]>([]);
  const [languages, setLanguages] = useState<LanguageOption[]>([]);
  const [data, setData] = useState<LeaderboardResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [historyRepo, setHistoryRepo] = useState<string | null>(null);
  const [theme, setTheme] = useState<Theme>(() => getPreferredTheme());
  const [density, setDensity] = useState<Density>(() => readDensity());
  const [showDesc, setShowDesc] = useState(() => readShowDesc());

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    applyDensityClasses(density, showDesc);
    writeDensity(density);
    writeShowDesc(showDesc);
  }, [density, showDesc]);

  useEffect(() => {
    const params = new URLSearchParams();
    params.set("board", board);
    if (board === "top") params.set("metric", metric);
    if (lang) params.set("lang", lang);
    if (date) params.set("date", date);
    window.history.replaceState(null, "", `?${params.toString()}`);
  }, [board, metric, lang, date]);

  // Backend board matching the current view; language counts are scoped per
  // board so they match what the filtered leaderboard actually returns.
  const backendBoard = board === "top" ? `top_${metric}` : "trending_daily";

  useEffect(() => {
    const dateQuery = date ? `&date=${encodeURIComponent(date)}` : "";
    api<LanguageOption[]>(`/api/languages?board=${backendBoard}${dateQuery}`)
      .then((ls) => {
        setLanguages(ls);
        // Drop the language filter if the selected language has no entries
        // on the newly selected board/date.
        setLang((cur) => (cur && !ls.some((l) => l.language === cur) ? "" : cur));
      })
      .catch((e) => {
        if (e instanceof UnauthorizedError) onUnauthorized?.();
      });
  }, [backendBoard, date, onUnauthorized]);

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
    const langQuery = lang ? `&language=${encodeURIComponent(lang)}` : "";
    const dateQuery = date ? `&date=${encodeURIComponent(date)}` : "";
    const path =
      board === "top"
        ? `/api/leaderboard/top?metric=${metric}${langQuery}${dateQuery}`
        : `/api/leaderboard/trending?x=1${langQuery}${dateQuery}`;
    try {
      setData(await api<LeaderboardResponse>(path));
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      setError(e instanceof Error ? e.message : "load failed");
    } finally {
      setLoading(false);
    }
  }, [board, metric, lang, date, onUnauthorized]);

  useEffect(() => {
    void load();
  }, [load]);

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
          {data?.date && <span>数据截至 {data.date}</span>}
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
          language={lang}
          languages={languages}
          date={date}
          dates={dates}
          density={density}
          showDesc={showDesc}
          onBoard={setBoard}
          onMetric={setMetric}
          onLanguage={setLang}
          onDate={setDate}
          onDensity={setDensity}
          onShowDesc={setShowDesc}
        />

        {loading && (
          <div className="space-y-2 py-4" aria-busy="true" aria-label="加载中">
            {[1, 2, 3, 4, 5].map((i) => (
              <div key={i} className="skeleton-row" style={{ opacity: 1 - i * 0.12 }} />
            ))}
          </div>
        )}
        {error && (
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
        {!loading && !error && data && (
          <LeaderboardTable
            items={data.items}
            board={board}
            metric={metric}
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
    </div>
  );
}
