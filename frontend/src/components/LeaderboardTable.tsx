import { useState } from "react";
import { compact } from "../api";
import { formatPct, langColor } from "../langColors";
import type { LanguageShare, LeaderboardItem } from "../types";
import type { BoardKind, Metric } from "../urlState";

interface Props {
  items: LeaderboardItem[];
  board: BoardKind;
  metric: Metric;
  density: "compact" | "comfortable";
  selectedTopics: string[];
  selectedLanguages: string[];
  onToggleTopic?: (topic: string) => void;
  onToggleLanguage?: (lang: string) => void;
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

function TopicRow({
  topics,
  selectedTopics,
  maxShow,
  onToggleTopic,
}: {
  topics: string[];
  selectedTopics: string[];
  maxShow: number;
  onToggleTopic?: (topic: string) => void;
}) {
  if (!topics.length) return null;
  const shown = topics.slice(0, maxShow);
  const rest = topics.length - shown.length;
  return (
    <div className="topic-row">
      {shown.map((t) => {
        const on = selectedTopics.includes(t);
        return (
          <button
            key={t}
            type="button"
            className={`chip-sm${on ? " on-filter" : ""}`}
            onClick={() => onToggleTopic?.(t)}
          >
            {t}
          </button>
        );
      })}
      {rest > 0 && (
        <span
          className="chip-sm more-topics"
          title={topics.slice(maxShow).join(", ")}
        >
          +{rest}
        </span>
      )}
    </div>
  );
}

function LanguagesCell({
  languages,
  maxShow,
  selectedLanguages,
  onToggleLanguage,
}: {
  languages: LanguageShare[];
  maxShow: number;
  selectedLanguages: string[];
  onToggleLanguage?: (lang: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);

  if (!languages.length) {
    return <div className="lang-empty">—</div>;
  }

  const rest = Math.max(0, languages.length - maxShow);
  const moreTitle = languages
    .slice(maxShow)
    .map((l) => `${l.name} ${formatPct(l.pct)}`)
    .join(" · ");

  return (
    <div className={`lang-stack${expanded ? " is-expanded" : ""}`}>
      <div className="lang-bar" role="img" aria-label="language breakdown">
        {languages.map((l) => (
          <span
            key={l.name}
            style={{
              width: `${Math.max(l.pct, 0.4)}%`,
              background: langColor(l.name),
            }}
            title={`${l.name} ${formatPct(l.pct)}`}
          />
        ))}
      </div>
      <div className="lang-list">
        {languages.map((l, i) => {
          const on = selectedLanguages.includes(l.name);
          const extra = i >= maxShow;
          return (
            <button
              key={l.name}
              type="button"
              className={`lang-row${on ? " on-filter" : ""}${extra ? " is-extra" : ""}`}
              onClick={() => onToggleLanguage?.(l.name)}
              title={`筛选：${l.name}`}
            >
              <span
                className="lang-dot"
                style={{ background: langColor(l.name) }}
              />
              <span className="lang-name">{l.name}</span>
              <span className="lang-pct">{formatPct(l.pct)}</span>
            </button>
          );
        })}
        {rest > 0 && (
          <button
            type="button"
            className="lang-more"
            aria-expanded={expanded}
            title={expanded ? "收起语言列表" : moreTitle}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setExpanded((v) => !v);
            }}
          >
            {expanded ? "收起" : `+${rest} more`}
          </button>
        )}
      </div>
    </div>
  );
}

export default function LeaderboardTable({
  items,
  board,
  metric,
  density,
  selectedTopics,
  selectedLanguages,
  onToggleTopic,
  onToggleLanguage,
  onSelectRepo,
}: Props) {
  if (items.length === 0) {
    return (
      <p className="py-8 text-center" style={{ color: "var(--muted)" }}>
        暂无数据（今日抓取可能未完成）
      </p>
    );
  }

  const maxTopics = density === "compact" ? 2 : 4;
  const maxLangs = density === "compact" ? 2 : 3;

  return (
    <div className="table-wrap">
      <table className="lb">
        <thead>
          <tr>
            <th style={{ width: 44 }}>#</th>
            <th>Repo</th>
            <th style={{ width: 200 }}>Languages</th>
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
            const topics = item.topics ?? [];
            const languages = item.languages ?? [];
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
                  <TopicRow
                    topics={topics}
                    selectedTopics={selectedTopics}
                    maxShow={maxTopics}
                    onToggleTopic={onToggleTopic}
                  />
                </td>
                <td className="lang-cell">
                  <LanguagesCell
                    languages={languages}
                    maxShow={maxLangs}
                    selectedLanguages={selectedLanguages}
                    onToggleLanguage={onToggleLanguage}
                  />
                </td>
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
