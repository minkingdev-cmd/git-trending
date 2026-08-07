export type BoardKind = "trending" | "top";
export type Metric = "stars" | "forks" | "watchers";
export type Density = "compact" | "comfortable";

interface Props {
  board: BoardKind;
  metric: Metric;
  language: string;
  languages: { language: string; count: number }[];
  date: string;
  dates: string[];
  density: Density;
  showDesc: boolean;
  onBoard: (b: BoardKind) => void;
  onMetric: (m: Metric) => void;
  onLanguage: (l: string) => void;
  onDate: (d: string) => void;
  onDensity: (d: Density) => void;
  onShowDesc: (show: boolean) => void;
}

export default function Controls({
  board,
  metric,
  language,
  languages,
  date,
  dates,
  density,
  showDesc,
  onBoard,
  onMetric,
  onLanguage,
  onDate,
  onDensity,
  onShowDesc,
}: Props) {
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
          value={language}
          onChange={(e) => onLanguage(e.target.value)}
          aria-label="语言"
        >
          <option value="">全部语言</option>
          {languages.map((l) => (
            <option key={l.language} value={l.language}>
              {l.language} ({l.count})
            </option>
          ))}
        </select>

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
    </section>
  );
}
