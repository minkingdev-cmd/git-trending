import { useEffect, useState } from "react";
import type { LanguageFacet, LicenseFacet, TopicFacet } from "../types";
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
  licenses: string[];
  excludeArchived: boolean;
  /** When set (e.g. 90), only repos active within N days. */
  activeWithin: number | null;
  topicFacets: TopicFacet[];
  languageFacets: LanguageFacet[];
  licenseFacets: LicenseFacet[];
  density: Density;
  showDesc: boolean;
  onBoard: (b: BoardKind) => void;
  onMetric: (m: Metric) => void;
  onDate: (d: string) => void;
  onQ: (q: string) => void;
  onToggleTopic: (topic: string) => void;
  onTopicMode: (mode: TopicMode) => void;
  onToggleLanguage: (lang: string) => void;
  onToggleLicense: (license: string) => void;
  onExcludeArchived: (v: boolean) => void;
  onActiveWithin: (days: number | null) => void;
  onClearFilters: () => void;
  onDensity: (d: Density) => void;
  onShowDesc: (show: boolean) => void;
}

const ACTIVE_WITHIN_DAYS = 90;

export default function Controls({
  board,
  metric,
  date,
  dates,
  q,
  topics,
  topicMode,
  languages,
  licenses,
  excludeArchived,
  activeWithin,
  topicFacets,
  languageFacets,
  licenseFacets,
  density,
  showDesc,
  onBoard,
  onMetric,
  onDate,
  onQ,
  onToggleTopic,
  onTopicMode,
  onToggleLanguage,
  onToggleLicense,
  onExcludeArchived,
  onActiveWithin,
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

  const hasFilters = Boolean(
    q.trim() || topics.length || languages.length || licenses.length,
  );
  // Facet chip rows collapse to a single line by default.
  const [langFacetsOpen, setLangFacetsOpen] = useState(false);
  const [topicFacetsOpen, setTopicFacetsOpen] = useState(false);
  const [licenseFacetsOpen, setLicenseFacetsOpen] = useState(false);

  return (
    <section className="controls-panel" aria-label="筛选">
      <div className="controls-row">
        <div className="seg" role="group" aria-label="榜单">
          {(
            [
              ["trending", "趋势榜"],
              ["top", "总榜"],
              ["tracked", "我的跟踪"],
              ["discover", "发现"],
            ] as const
          ).map(([b, label]) => (
            <button
              key={b}
              type="button"
              onClick={() => onBoard(b)}
              className={board === b ? "active" : undefined}
            >
              {label}
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

        {board !== "tracked" && board !== "discover" && (
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
        )}

        {/* Density / desc toggles stay available on all boards including discover. */}
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
      </div>

      {/* Board-local filters: hidden on discover (DiscoverPanel owns its controls). */}
      {board !== "discover" && (
        <>
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
              title="默认排除已归档仓库；取消勾选可显示 archived"
            >
              <input
                type="checkbox"
                checked={excludeArchived}
                onChange={(e) => onExcludeArchived(e.target.checked)}
              />
              排除已归档
            </label>
            <label
              className="toggle-chip"
              title={`仅显示最近 ${ACTIVE_WITHIN_DAYS} 天内有 push 且未归档的仓库`}
            >
              <input
                type="checkbox"
                checked={activeWithin === ACTIVE_WITHIN_DAYS}
                onChange={(e) =>
                  onActiveWithin(e.target.checked ? ACTIVE_WITHIN_DAYS : null)
                }
              />
              仅活跃({ACTIVE_WITHIN_DAYS}天)
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

          <div
            className={`facet-row${langFacetsOpen ? " is-expanded" : ""}`}
          >
            <span className="facet-label">语言</span>
            <div className="facet-chips-wrap">
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
            {languageFacets.length > 0 && (
              <button
                type="button"
                className="facet-toggle"
                onClick={() => setLangFacetsOpen((v) => !v)}
                aria-expanded={langFacetsOpen}
              >
                {langFacetsOpen ? "收起" : "展开"}
              </button>
            )}
          </div>

          <div
            className={`facet-row${topicFacetsOpen ? " is-expanded" : ""}`}
          >
            <span className="facet-label">标签</span>
            <div className="facet-chips-wrap">
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
            {topicFacets.length > 0 && (
              <button
                type="button"
                className="facet-toggle"
                onClick={() => setTopicFacetsOpen((v) => !v)}
                aria-expanded={topicFacetsOpen}
              >
                {topicFacetsOpen ? "收起" : "展开"}
              </button>
            )}
          </div>

          <div
            className={`facet-row${licenseFacetsOpen ? " is-expanded" : ""}`}
          >
            <span className="facet-label">许可</span>
            <div className="facet-chips-wrap">
              <div className="chips" aria-label="License 多选（OR）">
                {licenseFacets.length === 0 ? (
                  <span className="facet-empty">—</span>
                ) : (
                  licenseFacets.map(({ license, count }) => {
                    const active = licenses.includes(license);
                    return (
                      <button
                        key={license}
                        type="button"
                        className={`chip${active ? " active" : ""}`}
                        onClick={() => onToggleLicense(license)}
                      >
                        {license}
                        <span className="count">{count}</span>
                      </button>
                    );
                  })
                )}
              </div>
            </div>
            {licenseFacets.length > 0 && (
              <button
                type="button"
                className="facet-toggle"
                onClick={() => setLicenseFacetsOpen((v) => !v)}
                aria-expanded={licenseFacetsOpen}
              >
                {licenseFacetsOpen ? "收起" : "展开"}
              </button>
            )}
          </div>
        </>
      )}
    </section>
  );
}
