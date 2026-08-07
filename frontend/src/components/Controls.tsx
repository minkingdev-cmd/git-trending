import { useEffect, useState } from "react";
import type { LanguageFacet, TopicFacet } from "../types";
import { langColor } from "../langColors";
import type { BoardKind, Metric, TopicMode } from "../urlState";

export type { BoardKind, Metric } from "../urlState";
export type Density = "compact" | "comfortable";

interface Props {
  board: BoardKind;
  metric: Metric;
  date: string;
  dates: string[];
  q: string;
  topics: string[];
  topicMode: TopicMode;
  languages: string[];
  topicFacets: TopicFacet[];
  languageFacets: LanguageFacet[];
  density: Density;
  showDesc: boolean;
  onBoard: (b: BoardKind) => void;
  onMetric: (m: Metric) => void;
  onDate: (d: string) => void;
  onQ: (q: string) => void;
  onToggleTopic: (topic: string) => void;
  onTopicMode: (mode: TopicMode) => void;
  onToggleLanguage: (lang: string) => void;
  onClearFilters: () => void;
  onDensity: (d: Density) => void;
  onShowDesc: (show: boolean) => void;
}

export default function Controls({
  board,
  metric,
  date,
  dates,
  q,
  topics,
  topicMode,
  languages,
  topicFacets,
  languageFacets,
  density,
  showDesc,
  onBoard,
  onMetric,
  onDate,
  onQ,
  onToggleTopic,
  onTopicMode,
  onToggleLanguage,
  onClearFilters,
  onDensity,
  onShowDesc,
}: Props) {
  // Local draft for debounced keyword input.
  const [qDraft, setQDraft] = useState(q);
  useEffect(() => {
    setQDraft(q);
  }, [q]);
  useEffect(() => {
    if (qDraft === q) return;
    const t = window.setTimeout(() => onQ(qDraft), 280);
    return () => window.clearTimeout(t);
  }, [qDraft, q, onQ]);

  const hasFilters = Boolean(q.trim() || topics.length || languages.length);

  return (
    <section className="controls-panel" aria-label="筛选">
      <div className="controls-row">
        <div className="seg" role="group" aria-label="榜单">
          {(["trending", "top"] as BoardKind[]).map((b) => (
            <button
              key={b}
              type="button"
              onClick={() => onBoard(b)}
              className={board === b ? "active" : undefined}
            >
              {b === "trending" ? "趋势榜" : "总榜"}
            </button>
          ))}
        </div>

        {board === "top" && (
          <div className="metric-group">
            {(["stars", "forks", "watchers"] as Metric[]).map((m) => (
              <label key={m}>
                <input
                  type="radio"
                  name="metric"
                  checked={metric === m}
                  onChange={() => onMetric(m)}
                />
                {m === "stars" ? "Star" : m === "forks" ? "Fork" : "Watch"}
              </label>
            ))}
          </div>
        )}

        <select
          className="field-select"
          value={date}
          onChange={(e) => onDate(e.target.value)}
          title="快照日期"
          aria-label="日期"
        >
          <option value="">最新</option>
          {dates.map((d) => (
            <option key={d} value={d}>
              {d}
            </option>
          ))}
        </select>
      </div>

      <div className="controls-row">
        <div className="search-wrap">
          <span className="search-icon" aria-hidden="true">
            ⌕
          </span>
          <input
            className="field-input"
            type="search"
            value={qDraft}
            onChange={(e) => setQDraft(e.target.value)}
            placeholder="关键词：名字 / 描述 / 标签…"
            autoComplete="off"
            spellCheck={false}
            aria-label="关键词"
          />
        </div>

        <div className="topic-mode" role="group" aria-label="标签匹配模式">
          <button
            type="button"
            className={topicMode === "and" ? "active" : undefined}
            onClick={() => onTopicMode("and")}
            title="必须同时包含所选标签"
          >
            AND
          </button>
          <button
            type="button"
            className={topicMode === "or" ? "active" : undefined}
            onClick={() => onTopicMode("or")}
            title="包含任一所选标签"
          >
            OR
          </button>
        </div>

        <label
          className="toggle-chip"
          title="紧凑模式减少行高，介绍保留一行"
        >
          <input
            type="checkbox"
            checked={density === "compact"}
            onChange={(e) =>
              onDensity(e.target.checked ? "compact" : "comfortable")
            }
          />
          紧凑密度
        </label>
        <label className="toggle-chip" title="关闭后隐藏仓库介绍">
          <input
            type="checkbox"
            checked={showDesc}
            onChange={(e) => onShowDesc(e.target.checked)}
          />
          显示介绍
        </label>

        <button
          type="button"
          className="clear-filters"
          onClick={onClearFilters}
          disabled={!hasFilters}
        >
          清除筛选
        </button>
      </div>

      <div className="lang-bar-filter">
        <span className="facet-label">语言</span>
        <div className="chips" aria-label="语言多选（仓库含该语言即匹配）">
          {languageFacets.length === 0 ? (
            <span className="facet-empty">—</span>
          ) : (
            languageFacets.map(({ language, count }) => {
              const active = languages.includes(language);
              return (
                <button
                  key={language}
                  type="button"
                  className={`chip${active ? " active" : ""}`}
                  onClick={() => onToggleLanguage(language)}
                >
                  <span
                    className="lang-dot"
                    style={{
                      background: langColor(language),
                      width: 7,
                      height: 7,
                    }}
                  />
                  {language}
                  <span className="count">{count}</span>
                </button>
              );
            })
          )}
        </div>
      </div>

      <div className="topic-bar">
        <span className="facet-label">标签</span>
        <div className="chips" aria-label="可选标签">
          {topicFacets.length === 0 ? (
            <span className="facet-empty">—</span>
          ) : (
            topicFacets.map(({ topic, count }) => {
              const active = topics.includes(topic);
              return (
                <button
                  key={topic}
                  type="button"
                  className={`chip${active ? " active" : ""}`}
                  onClick={() => onToggleTopic(topic)}
                >
                  {topic}
                  <span className="count">{count}</span>
                </button>
              );
            })
          )}
        </div>
      </div>
    </section>
  );
}
