import { useCallback, useEffect, useState } from "react";
import Controls, { type BoardKind, type Metric } from "./Controls";
import LeaderboardTable from "./LeaderboardTable";
import RepoHistoryPanel from "./RepoHistoryPanel";
import { api } from "../api";
import type { LanguageOption, LeaderboardResponse, MetaResponse } from "../types";

interface Props {
  username: string;
  isAdmin: boolean;
  onLogout: () => void;
  onOpenAdmin?: () => void;
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

export default function Leaderboard({ username, isAdmin, onLogout, onOpenAdmin }: Props) {
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

  useEffect(() => {
    const params = new URLSearchParams();
    params.set("board", board);
    if (board === "top") params.set("metric", metric);
    if (lang) params.set("lang", lang);
    if (date) params.set("date", date);
    window.history.replaceState(null, "", `?${params.toString()}`);
  }, [board, metric, lang, date]);

  useEffect(() => {
    api<LanguageOption[]>("/api/languages")
      .then(setLanguages)
      .catch(() => {});
    api<MetaResponse>("/api/meta")
      .then((m) => setDates(m.dates ?? []))
      .catch(() => {});
  }, []);

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
      setError(e instanceof Error ? e.message : "load failed");
    } finally {
      setLoading(false);
    }
  }, [board, metric, lang, date]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <div className="mx-auto max-w-5xl space-y-4 p-6">
        <header className="flex items-center justify-between">
          <h1 className="text-xl font-semibold">GH Trending</h1>
          <div className="flex items-center gap-3 text-sm text-neutral-400">
            {data?.date && <span>数据截至 {data.date}</span>}
            <span>{username}</span>
            {isAdmin && onOpenAdmin && (
              <button
                type="button"
                onClick={onOpenAdmin}
                className="text-neutral-300 hover:text-white"
              >
                管理
              </button>
            )}
            <button type="button" onClick={onLogout} className="text-neutral-300 hover:text-white">
              登出
            </button>
          </div>
        </header>

        <Controls
          board={board}
          metric={metric}
          language={lang}
          languages={languages}
          date={date}
          dates={dates}
          onBoard={setBoard}
          onMetric={setMetric}
          onLanguage={setLang}
          onDate={setDate}
        />

        {loading && <p className="py-8 text-center text-neutral-500">加载中…</p>}
        {error && <p className="py-8 text-center text-red-400">{error}</p>}
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
        />
      )}
    </div>
  );
}
