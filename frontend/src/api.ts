import type {
  DiscoverSearchResponse,
  GithubTokenStatus,
  LookupResponse,
  RepoRefBody,
  TrackedListResponse,
  TrackedRepoItem,
  TrackResponse,
} from "./types";

export class UnauthorizedError extends Error {
  constructor() {
    super("unauthorized");
  }
}

export class ApiError extends Error {
  status: number;
  body: unknown;

  constructor(status: number, message: string, body?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }
}

/** Typed fields often present on discover / rate-limit error bodies. */
export interface ApiErrorBody {
  error?: string;
  message?: string;
  scope?: string;
  auth_mode?: string;
  retry_after_secs?: number;
}
let refreshInFlight: Promise<boolean> | null = null;

export function tryRefresh(): Promise<boolean> {
  if (!refreshInFlight) {
    refreshInFlight = fetch("/api/auth/refresh", { method: "POST", credentials: "include" })
      .then((r) => r.ok)
      .catch(() => false)
      .finally(() => {
        refreshInFlight = null;
      });
  }
  return refreshInFlight;
}

async function readErrorPayload(
  res: Response,
): Promise<{ message: string; body: unknown }> {
  try {
    const text = await res.text();
    if (!text) return { message: `HTTP ${res.status}`, body: undefined };
    try {
      const json = JSON.parse(text) as ApiErrorBody;
      const message =
        (typeof json.error === "string" && json.error) ||
        (typeof json.message === "string" && json.message) ||
        text.slice(0, 200);
      return { message, body: json };
    } catch {
      return { message: text.slice(0, 200), body: undefined };
    }
  } catch {
    return { message: `HTTP ${res.status}`, body: undefined };
  }
}

/** Low-level fetch with credentials + single 401 refresh retry. */
export async function apiFetch(path: string, init?: RequestInit): Promise<Response> {
  const doFetch = () =>
    fetch(path, {
      credentials: "include",
      ...init,
    });
  let res = await doFetch();
  if (res.status === 401 && !path.startsWith("/api/auth/")) {
    const refreshed = await tryRefresh();
    if (refreshed) {
      res = await doFetch();
    }
  }
  return res;
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await apiFetch(path, init);
  if (res.status === 401) {
    throw new UnauthorizedError();
  }
  if (!res.ok) {
    const { message, body } = await readErrorPayload(res);
    throw new ApiError(res.status, message, body);
  }
  if (res.status === 204) {
    return undefined as T;
  }
  return res.json() as Promise<T>;
}

/**
 * Parse `owner/name` or a github.com URL into `owner/name`.
 * Returns null when input is empty or not a valid public GitHub repo ref.
 */
export function parseRepoRef(raw: string): string | null {
  const s = raw.trim();
  if (!s) return null;

  let path = s;
  const lower = s.toLowerCase();
  const prefixes = [
    "https://github.com/",
    "http://github.com/",
    "https://www.github.com/",
    "http://www.github.com/",
    "github.com/",
    "www.github.com/",
  ];
  let stripped = false;
  for (const p of prefixes) {
    if (lower.startsWith(p)) {
      path = s.slice(p.length);
      stripped = true;
      break;
    }
  }

  if (!stripped) {
    // Reject non-github hosts / schemes.
    if (
      s.includes("://") ||
      s.startsWith("www.") ||
      (s.includes(".") &&
        (s.split("/")[0]?.includes(".") ?? false))
    ) {
      return null;
    }
  }

  path = path.split(/[?#]/)[0] ?? path;
  path = path.replace(/^\/+|\/+$/g, "");
  if (path.toLowerCase().endsWith(".git")) {
    path = path.slice(0, -4);
  }

  const parts = path.split("/").filter(Boolean);
  if (parts.length !== 2) return null;
  const [owner, name] = parts;
  if (!owner || !name) return null;
  if (!isValidGithubSegment(owner) || !isValidGithubSegment(name)) return null;
  return `${owner}/${name}`;
}

function isValidGithubSegment(s: string): boolean {
  return (
    s.length > 0 &&
    s.length <= 100 &&
    /^[A-Za-z0-9._-]+$/.test(s)
  );
}

/** Build lookup/track body from free-form input. */
export function repoRefBodyFromInput(raw: string): RepoRefBody | null {
  const s = raw.trim();
  if (!s) return null;
  const fullName = parseRepoRef(s);
  if (!fullName) return null;
  // Prefer full_name; also accept original URL shape when user pasted a URL.
  if (/github\.com/i.test(s) || s.includes("://")) {
    return { url: s, full_name: fullName };
  }
  return { full_name: fullName };
}

export async function lookupRepo(body: RepoRefBody): Promise<LookupResponse> {
  return api<LookupResponse>("/api/repos/lookup", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

export async function trackRepo(body: RepoRefBody): Promise<TrackedRepoItem> {
  const res = await api<TrackResponse>("/api/repos/track", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return res.item;
}

export async function untrackRepo(fullName: string): Promise<void> {
  await api<void>(
    `/api/repos/track?full_name=${encodeURIComponent(fullName)}`,
    { method: "DELETE" },
  );
}

export interface ListTrackedParams {
  q?: string;
  topics?: string[];
  languages?: string[];
  licenses?: string[];
  topicMode?: "and" | "or";
  /** Default true on server; pass false to include archived. */
  excludeArchived?: boolean;
  /** Only repos with pushed_at within N days. */
  activeWithin?: number | null;
}

/** Append health / board filter params (shared by leaderboard + tracked). */
export function appendHealthFilterParams(
  qs: URLSearchParams,
  opts: { excludeArchived?: boolean; activeWithin?: number | null },
): void {
  // Server defaults exclude_archived=true; only send when disabling.
  if (opts.excludeArchived === false) {
    qs.set("exclude_archived", "0");
  }
  if (opts.activeWithin != null && opts.activeWithin > 0) {
    qs.set("active_within", String(opts.activeWithin));
  }
}

export async function listTrackedRepos(
  params: ListTrackedParams = {},
): Promise<TrackedRepoItem[]> {
  const qs = new URLSearchParams();
  if (params.q?.trim()) qs.set("q", params.q.trim());
  if (params.topics?.length) qs.set("topics", params.topics.join(","));
  if (params.languages?.length) qs.set("languages", params.languages.join(","));
  if (params.licenses?.length) qs.set("licenses", params.licenses.join(","));
  if (params.topicMode && params.topicMode !== "and") {
    qs.set("topic_mode", params.topicMode);
  }
  appendHealthFilterParams(qs, {
    excludeArchived: params.excludeArchived,
    activeWithin: params.activeWithin,
  });
  const q = qs.toString();
  const res = await api<TrackedListResponse>(
    `/api/repos/tracked${q ? `?${q}` : ""}`,
  );
  return res.items ?? [];
}

export interface DiscoverSearchParams {
  q?: string;
  language?: string;
  license?: string;
  minStars?: number | null;
  /** Default true on server; pass false to include archived. */
  excludeArchived?: boolean;
  activeWithin?: number | null;
  sort?: "stars" | "updated";
  page?: number;
}

/** GET /api/discover/search — proxies GitHub Search (does not write DB). */
export async function discoverSearch(
  params: DiscoverSearchParams = {},
): Promise<DiscoverSearchResponse> {
  const qs = new URLSearchParams();
  if (params.q?.trim()) qs.set("q", params.q.trim());
  if (params.language?.trim()) qs.set("language", params.language.trim());
  if (params.license?.trim()) qs.set("license", params.license.trim());
  if (params.minStars != null && params.minStars >= 0) {
    qs.set("min_stars", String(params.minStars));
  }
  appendHealthFilterParams(qs, {
    excludeArchived: params.excludeArchived,
    activeWithin: params.activeWithin,
  });
  if (params.sort && params.sort !== "stars") {
    qs.set("sort", params.sort);
  }
  if (params.page != null && params.page > 1) {
    qs.set("page", String(params.page));
  }
  const q = qs.toString();
  return api<DiscoverSearchResponse>(
    `/api/discover/search${q ? `?${q}` : ""}`,
  );
}

/** PUT /api/me/github-token — save encrypted personal PAT (never echoed). */
export async function putGithubToken(token: string): Promise<GithubTokenStatus> {
  return api<GithubTokenStatus>("/api/me/github-token", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token }),
  });
}

/** DELETE /api/me/github-token — clear stored PAT. */
export async function deleteGithubToken(): Promise<GithubTokenStatus> {
  return api<GithubTokenStatus>("/api/me/github-token", {
    method: "DELETE",
  });
}

export async function postAuth(path: string, body?: unknown): Promise<Response> {
  return fetch(path, {
    method: "POST",
    credentials: "include",
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
}

const REFRESH_INTERVAL_MS = 10 * 60 * 1000;

export function startRefreshTimer(onExpired: () => void): () => void {
  const tick = () => {
    if (document.visibilityState !== "visible") return;
    void tryRefresh().then((ok) => {
      if (!ok) onExpired();
    });
  };
  const timer = window.setInterval(tick, REFRESH_INTERVAL_MS);
  document.addEventListener("visibilitychange", tick);
  return () => {
    window.clearInterval(timer);
    document.removeEventListener("visibilitychange", tick);
  };
}

export function compact(n: number): string {
  return new Intl.NumberFormat("en", { notation: "compact" }).format(n);
}
