import { compact } from "../api";
import type { LeaderboardItem } from "../types";
import type { BoardKind, Metric } from "./Controls";

interface Props {
  items: LeaderboardItem[];
  board: BoardKind;
  metric: Metric;
  onSelectRepo?: (fullName: string) => void;
}

export default function LeaderboardTable({ items, board, metric, onSelectRepo }: Props) {
  if (items.length === 0) {
    return (
      <p className="py-8 text-center text-neutral-500">暂无数据（今日抓取可能未完成）</p>
    );
  }
  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="border-b border-neutral-800 text-left text-neutral-500">
          <th className="py-2 pr-2 w-10">#</th>
          <th className="py-2 pr-2">Repo</th>
          <th className="py-2 pr-2">Language</th>
          {board === "trending" ? (
            <>
              <th className="py-2 pr-2 text-right">★ today</th>
              <th className="py-2 pr-2 text-right">★</th>
              <th className="py-2 text-right">Fork</th>
            </>
          ) : (
            <>
              <th className="py-2 pr-2 text-right">
                {metric === "stars" ? "★" : metric === "forks" ? "Fork" : "Watch"}
              </th>
              <th className="py-2 pr-2 text-right">★</th>
              <th className="py-2 text-right">Fork</th>
            </>
          )}
        </tr>
      </thead>
      <tbody>
        {items.map((item) => (
          <tr key={item.full_name} className="border-b border-neutral-900 hover:bg-neutral-900/50">
            <td className="py-2 pr-2 text-neutral-500">{item.rank}</td>
            <td className="py-2 pr-2">
              <div className="flex flex-wrap items-center gap-2">
                <a
                  href={item.html_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-emerald-400 hover:underline"
                  title={item.description ?? undefined}
                >
                  {item.full_name}
                </a>
                {onSelectRepo && (
                  <button
                    type="button"
                    onClick={() => onSelectRepo(item.full_name)}
                    className="text-xs text-neutral-500 hover:text-neutral-200"
                    title="查看历史趋势"
                  >
                    趋势
                  </button>
                )}
              </div>
            </td>
            <td className="py-2 pr-2 text-neutral-400">{item.language ?? "—"}</td>
            {board === "trending" ? (
              <>
                <td className="py-2 pr-2 text-right font-medium">
                  {compact(item.stars_today ?? 0)}
                </td>
                <td className="py-2 pr-2 text-right text-neutral-300">{compact(item.stars)}</td>
                <td className="py-2 text-right text-neutral-300">{compact(item.forks)}</td>
              </>
            ) : (
              <>
                <td className="py-2 pr-2 text-right font-medium">
                  {compact(
                    metric === "stars"
                      ? item.stars
                      : metric === "forks"
                        ? item.forks
                        : (item.watchers ?? 0),
                  )}
                </td>
                <td className="py-2 pr-2 text-right text-neutral-300">{compact(item.stars)}</td>
                <td className="py-2 text-right text-neutral-300">{compact(item.forks)}</td>
              </>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
