import { useEffect, useState } from "react";
import { api, UnauthorizedError } from "../api";
import type { HistoryResponse } from "../types";
import Sparkline from "./Sparkline";
import type { BoardKind, Metric } from "./Controls";

interface Props {
  fullName: string;
  board: BoardKind;
  metric: Metric;
  onClose: () => void;
  onUnauthorized?: () => void;
}

function historyBoard(board: BoardKind, metric: Metric): string {
  if (board === "trending") return "trending_daily";
  if (metric === "forks") return "top_forks";
  if (metric === "watchers") return "top_watchers";
  return "top_stars";
}

function valueFromPoint(
  p: HistoryResponse["points"][0],
  board: BoardKind,
  metric: Metric,
): number {
  if (board === "trending") return p.stars_today ?? 0;
  if (metric === "forks") return p.forks;
  if (metric === "watchers") return p.watchers ?? 0;
  return p.stars;
}

export default function RepoHistoryPanel({
  fullName,
  board,
  metric,
  onClose,
  onUnauthorized,
}: Props) {
  const [data, setData] = useState<HistoryResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    setError(null);
    const b = historyBoard(board, metric);
    api<HistoryResponse>(
      `/api/repo/history?full_name=${encodeURIComponent(fullName)}&board=${b}&days=90`,
    )
      .then(setData)
      .catch((e) => {
        if (e instanceof UnauthorizedError) {
          onUnauthorized?.();
          return;
        }
        setError(e instanceof Error ? e.message : "load failed");
      })
      .finally(() => setLoading(false));
  }, [fullName, board, metric, onUnauthorized]);

  const points =
    data?.points.map((p) => ({
      date: p.date,
      value: valueFromPoint(p, board, metric),
    })) ?? [];

  return (
    <div className="fixed inset-0 z-50 flex items-end sm:items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-md rounded-lg border border-neutral-800 bg-neutral-950 p-4 shadow-xl">
        <div className="mb-3 flex items-start justify-between gap-3">
          <div>
            <h2 className="font-medium text-neutral-100">{fullName}</h2>
            <p className="text-xs text-neutral-500">近 90 天快照趋势</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-neutral-400 hover:text-white text-sm"
          >
            关闭
          </button>
        </div>
        {loading && <p className="py-6 text-center text-neutral-500 text-sm">加载中…</p>}
        {error && <p className="py-6 text-center text-red-400 text-sm">{error}</p>}
        {!loading && !error && <Sparkline points={points} />}
        <a
          href={`https://github.com/${fullName}`}
          target="_blank"
          rel="noopener noreferrer"
          className="mt-3 block text-center text-sm text-emerald-400 hover:underline"
        >
          在 GitHub 打开
        </a>
      </div>
    </div>
  );
}
