export type BoardKind = "trending" | "top" | "tracked";
export type Metric = "stars" | "forks" | "watchers";
export type TopicMode = "and" | "or";

export interface UrlState {
  board: BoardKind;
  metric: Metric;
  date: string;
  q: string;
  topics: string[];
  topicMode: TopicMode;
  languages: string[];
  licenses: string[];
}

export const DEFAULT_URL_STATE: UrlState = {
  board: "trending",
  metric: "stars",
  date: "",
  q: "",
  topics: [],
  topicMode: "and",
  languages: [],
  licenses: [],
};

/** Parse comma-separated list; trim, drop empty; optionally lowercase; dedupe. */
export function parseCsvList(raw: string | null, lowercase = false): string[] {
  if (!raw) return [];
  const parts = raw
    .split(",")
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
    .map((p) => (lowercase ? p.toLowerCase() : p));
  const seen = new Set<string>();
  const out: string[] = [];
  for (const p of parts) {
    if (seen.has(p)) continue;
    seen.add(p);
    out.push(p);
  }
  return out;
}

export function toCsv(list: string[]): string {
  return list.join(",");
}

/** Toggle membership of `value` in `list` (immutable). */
export function toggleInList(list: string[], value: string): string[] {
  const i = list.indexOf(value);
  if (i >= 0) return list.filter((_, idx) => idx !== i);
  return [...list, value];
}

/**
 * Parse `window.location.search` (or any search string, with or without `?`).
 * Accepts legacy `lang` as a single-language filter when `languages` is absent.
 */
export function parseSearch(search: string): UrlState {
  const raw = search.startsWith("?") ? search.slice(1) : search;
  const params = new URLSearchParams(raw);

  const boardRaw = params.get("board");
  const board: BoardKind =
    boardRaw === "top" || boardRaw === "tracked" ? boardRaw : "trending";
  const metricRaw = params.get("metric");
  const metric: Metric =
    metricRaw === "forks" || metricRaw === "watchers" ? metricRaw : "stars";

  const languagesCsv = params.get("languages");
  let languages = parseCsvList(languagesCsv, false);
  if (languages.length === 0) {
    const legacy = params.get("lang")?.trim();
    if (legacy) languages = [legacy];
  }

  const topicModeRaw = params.get("topic_mode")?.trim().toLowerCase();
  const topicMode: TopicMode = topicModeRaw === "or" ? "or" : "and";

  return {
    board,
    metric,
    date: params.get("date") ?? "",
    q: params.get("q") ?? "",
    topics: parseCsvList(params.get("topics"), true),
    topicMode,
    languages,
    licenses: parseCsvList(params.get("licenses"), false),
  };
}

/** Build a search string starting with `?` (empty → `?` with no params omitted). */
export function buildSearch(state: UrlState): string {
  const params = new URLSearchParams();
  params.set("board", state.board);
  if (state.board === "top") {
    params.set("metric", state.metric);
  }
  if (state.date) params.set("date", state.date);
  if (state.q.trim()) params.set("q", state.q.trim());
  if (state.topics.length) params.set("topics", toCsv(state.topics));
  if (state.topicMode !== "and") params.set("topic_mode", state.topicMode);
  if (state.languages.length) params.set("languages", toCsv(state.languages));
  if (state.licenses.length) params.set("licenses", toCsv(state.licenses));
  const s = params.toString();
  return s ? `?${s}` : "?";
}

/** True when keyword / topics / languages / licenses filters are active. */
export function hasActiveFilters(
  state: Pick<UrlState, "q" | "topics" | "languages" | "licenses">,
): boolean {
  return Boolean(
    state.q.trim() ||
      state.topics.length ||
      state.languages.length ||
      state.licenses.length,
  );
}
