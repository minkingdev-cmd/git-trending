import { compact } from "../api";
import type { LeaderboardItem } from "../types";
import type { BoardKind, Metric } from "./Controls";

interface Props {
  items: LeaderboardItem[];
  board: BoardKind;
  metric: Metric;
  onSelectRepo?: (fullName: string) => void;
}

/** Primary column header label for the current board/metric. */
export function primaryHeader(board: BoardKind, metric: Metric): string {
  if (board === "trending") return "今日 ★";
  if (metric === "forks") return "Fork";
  if (metric === "watchers") return "Watch";
  return "★";
}

/** Primary metric value for a row. */
export function primaryValue(
  item: LeaderboardItem,
  board: BoardKind,
  metric: Metric,
): number {
  if (board === "trending") return item.stars_today ?? 0;
  if (metric === "forks") return item.forks;
  if (metric === "watchers") return item.watchers ?? 0;
  return item.stars;
}

/**
 * Secondary metrics as a single merged cell — avoids dual ★ headers on top board.
 * Trending: total stars + forks; top board: the two non-primary metrics.
 */
export function secondaryLabel(
  item: LeaderboardItem,
  board: BoardKind,
  metric: Metric,
): string {
  if (board === "trending") {
    return `${compact(item.stars)} ★ · ${compact(item.forks)} ⑂`;
  }
  if (metric === "stars") {
    return `${compact(item.forks)} ⑂ · ${compact(item.watchers ?? 0)} 👁`;
  }
  if (metric === "forks") {
    return `${compact(item.stars)} ★ · ${compact(item.watchers ?? 0)} 👁`;
  }
  return `${compact(item.stars)} ★ · ${compact(item.forks)} ⑂`;
}

function splitName(fullName: string): { owner: string; name: string } {
  const i = fullName.indexOf("/");
  if (i <= 0) return { owner: "", name: fullName };
  return { owner: fullName.slice(0, i), name: fullName.slice(i + 1) };
}

function rankClass(rank: number): string {
  if (rank === 1) return "rank t1";
  if (rank === 2) return "rank t2";
  if (rank === 3) return "rank t3";
  return "rank";
}

export default function LeaderboardTable({
  items,
  board,
  metric,
  onSelectRepo,
}: Props) {
  if (items.length === 0) {
    return (
      <p className="py-8 text-center" style={{ color: "var(--muted)" }}>
        暂无数据（今日抓取可能未完成）
      </p>
    );
  }

  return (
    <div className="table-wrap">
      <table className="lb">
        <thead>
          <tr>
            <th style={{ width: 44 }}>#</th>
            <th>Repo</th>
            <th style={{ width: 120 }}>Lang</th>
            <th className="num" style={{ width: 100 }}>
              {primaryHeader(board, metric)}
            </th>
            <th className="num" style={{ width: 160 }}>
              辅指标
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const { owner, name } = splitName(item.full_name);
            const primary = primaryValue(item, board, metric);
            return (
              <tr key={item.full_name}>
                <td className={rankClass(item.rank)}>{item.rank}</td>
                <td className="repo-cell">
                  <div className="repo-head">
                    <div className="repo-name">
                      <a
                        href={item.html_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        title={item.full_name}
                      >
                        {owner ? (
                          <>
                            <span className="owner">{owner}</span>
                            <span className="slash">/</span>
                            <span className="name">{name}</span>
                          </>
                        ) : (
                          <span className="name">{item.full_name}</span>
                        )}
                      </a>
                    </div>
                    {onSelectRepo && (
                      <div className="row-actions">
                        <button
                          type="button"
                          onClick={() => onSelectRepo(item.full_name)}
                          title="查看历史趋势"
                          aria-label={`查看 ${item.full_name} 历史趋势`}
                        >
                          📈
                        </button>
                      </div>
                    )}
                  </div>
                  {item.description && (
                    <p className="repo-desc" title={item.description}>
                      {item.description}
                    </p>
                  )}
                </td>
                <td className="lang-cell">{item.language ?? "—"}</td>
                <td className="num metric-primary">
                  {board === "trending" ? (
                    <span className="plus">+{compact(primary)}</span>
                  ) : (
                    compact(primary)
                  )}
                </td>
                <td className="num metric-secondary">
                  {secondaryLabel(item, board, metric)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
