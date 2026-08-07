export type BoardKind = "trending" | "top" | "tracked" | "discover";
export type Metric = "stars" | "forks" | "watchers";
export type TopicMode = "and" | "or";
export type DiscoverSort = "stars" | "updated";

export interface UrlState {
  board: BoardKind;
  metric: Metric;
  date: string;
  q: string;
  topics: string[];
  topicMode: TopicMode;
  languages: string[];
  licenses: string[];
  /** Default true: exclude archived repos. URL: omit or 1; pass exclude_archived=0 to disable. */
  excludeArchived: boolean;
  /** Optional day window for pushed_at; null = no filter. URL: active_within=N. */
  activeWithin: number | null;
  // --- Discover namespace (d* params; only written when board=discover) ---
  dq: string;
  dlanguage: string;
  dlicense: string;
  dminStars: number | null;
  /** Default true. URL: omit or 1; pass dexclude_archived=0 to disable. */
  dexcludeArchived: boolean;
  dactiveWithin: number | null;
  dsort: DiscoverSort;
  /** 1-based page; default 1. */
  dpage: number;
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
  excludeArchived: true,
  activeWithin: null,
  dq: "",
  dlanguage: "",
  dlicense: "",
  dminStars: null,
  dexcludeArchived: true,
  dactiveWithin: null,
  dsort: "stars",
  dpage: 1,
};

/** Parse exclude_archived: missing/empty/1/true → true; 0/false/no → false. */
export function parseExcludeArchived(raw: string | null): boolean {
  if (raw == null) return true;
  const s = raw.trim().toLowerCase();
  if (s === "" || s === "1" || s === "true" || s === "yes") return true;
  if (s === "0" || s === "false" || s === "no") return false;
  return true;
}

/** Parse active_within days; missing/empty/invalid/non-positive → null. */
export function parseActiveWithin(raw: string | null): number | null {
  if (raw == null) return null;
  const s = raw.trim();
  if (!s) return null;
  const n = Number.parseInt(s, 10);
  if (!Number.isFinite(n) || n <= 0) return null;
  return n;
}

/** Parse non-negative integer (for min_stars); invalid → null. */
export function parseNonNegInt(raw: string | null): number | null {
  if (raw == null) return null;
  const s = raw.trim();
  if (!s) return null;
  const n = Number.parseInt(s, 10);
  if (!Number.isFinite(n) || n < 0) return null;
  return n;
}

/** Parse discover page 1..=10; missing/invalid → 1. */
export function parseDiscoverPage(raw: string | null): number {
  if (raw == null) return 1;
  const s = raw.trim();
  if (!s) return 1;
  const n = Number.parseInt(s, 10);
  if (!Number.isFinite(n) || n < 1) return 1;
  if (n > 10) return 10;
  return n;
}

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
 * True when discover has at least one search condition
 * (exclude_archived alone does not count — matches API).
 */
export function hasDiscoverCondition(
  state: Pick<
    UrlState,
    "dq" | "dlanguage" | "dlicense" | "dminStars" | "dactiveWithin"
  >,
): boolean {
  return Boolean(
    state.dq.trim() ||
      state.dlanguage.trim() ||
      state.dlicense.trim() ||
      (state.dminStars != null && state.dminStars >= 0) ||
      (state.dactiveWithin != null && state.dactiveWithin > 0),
  );
}

/**
 * Parse `window.location.search` (or any search string, with or without `?`).
 * Accepts legacy `lang` as a single-language filter when `languages` is absent.
 * Discover `d*` params are always read; they are only written when board=discover.
 */
export function parseSearch(search: string): UrlState {
  const raw = search.startsWith("?") ? search.slice(1) : search;
  const params = new URLSearchParams(raw);

  const boardRaw = params.get("board");
  const board: BoardKind =
    boardRaw === "top" || boardRaw === "tracked" || boardRaw === "discover"
      ? boardRaw
      : "trending";
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

  const dsortRaw = params.get("dsort")?.trim().toLowerCase();
  const dsort: DiscoverSort = dsortRaw === "updated" ? "updated" : "stars";

  return {
    board,
    metric,
    date: params.get("date") ?? "",
    q: params.get("q") ?? "",
    topics: parseCsvList(params.get("topics"), true),
    topicMode,
    languages,
    licenses: parseCsvList(params.get("licenses"), false),
    excludeArchived: parseExcludeArchived(params.get("exclude_archived")),
    activeWithin: parseActiveWithin(params.get("active_within")),
    dq: params.get("dq") ?? "",
    dlanguage: params.get("dlanguage") ?? "",
    dlicense: params.get("dlicense") ?? "",
    dminStars: parseNonNegInt(params.get("dmin_stars")),
    dexcludeArchived: parseExcludeArchived(params.get("dexclude_archived")),
    dactiveWithin: parseActiveWithin(params.get("dactive_within")),
    dsort,
    dpage: parseDiscoverPage(params.get("dpage")),
  };
}

/** Build a search string starting with `?`. Discover `d*` only when board=discover (drop on leave). */
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
  // Default excludeArchived=true is omitted; only write when disabled.
  if (!state.excludeArchived) params.set("exclude_archived", "0");
  if (state.activeWithin != null && state.activeWithin > 0) {
    params.set("active_within", String(state.activeWithin));
  }

  // Discover namespace: only when on discover tab (spec: drop d* when leaving).
  if (state.board === "discover") {
    if (state.dq.trim()) params.set("dq", state.dq.trim());
    if (state.dlanguage.trim()) params.set("dlanguage", state.dlanguage.trim());
    if (state.dlicense.trim()) params.set("dlicense", state.dlicense.trim());
    if (state.dminStars != null && state.dminStars >= 0) {
      params.set("dmin_stars", String(state.dminStars));
    }
    if (!state.dexcludeArchived) params.set("dexclude_archived", "0");
    if (state.dactiveWithin != null && state.dactiveWithin > 0) {
      params.set("dactive_within", String(state.dactiveWithin));
    }
    if (state.dsort !== "stars") params.set("dsort", state.dsort);
    if (state.dpage > 1) params.set("dpage", String(state.dpage));
  }

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
