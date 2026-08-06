export type BoardKind = "trending" | "top";
export type Metric = "stars" | "forks" | "watchers";

interface Props {
  board: BoardKind;
  metric: Metric;
  language: string;
  languages: { language: string; count: number }[];
  onBoard: (b: BoardKind) => void;
  onMetric: (m: Metric) => void;
  onLanguage: (l: string) => void;
}

export default function Controls({
  board,
  metric,
  language,
  languages,
  onBoard,
  onMetric,
  onLanguage,
}: Props) {
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-neutral-800 pb-4">
      <div className="flex gap-1 rounded-md bg-neutral-900 p-1">
        {(["trending", "top"] as BoardKind[]).map((b) => (
          <button
            key={b}
            type="button"
            onClick={() => onBoard(b)}
            className={`rounded px-3 py-1 text-sm ${
              board === b ? "bg-emerald-600 text-white" : "text-neutral-400 hover:text-neutral-200"
            }`}
          >
            {b === "trending" ? "趋势榜" : "总榜"}
          </button>
        ))}
      </div>

      {board === "top" && (
        <div className="flex gap-3 text-sm">
          {(["stars", "forks", "watchers"] as Metric[]).map((m) => (
            <label key={m} className="flex items-center gap-1 text-neutral-300">
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
        className="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-sm"
        value={language}
        onChange={(e) => onLanguage(e.target.value)}
      >
        <option value="">全部语言</option>
        {languages.map((l) => (
          <option key={l.language} value={l.language}>
            {l.language} ({l.count})
          </option>
        ))}
      </select>
    </div>
  );
}
