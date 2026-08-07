import type {
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

async function readErrorMessage(res: Response): Promise<string> {
  try {
    const text = await res.text();
    if (!text) return `HTTP ${res.status}`;
    try {
      const json = JSON.parse(text) as { error?: string; message?: string };
      if (typeof json.error === "string" && json.error) return json.error;
      if (typeof json.message === "string" && json.message) return json.message;
    } catch {
      /* not json */
    }
    return text.slice(0, 200);
  } catch {
    return `HTTP ${res.status}`;
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
    const msg = await readErrorMessage(res);
    throw new ApiError(res.status, msg);
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
  const q = qs.toString();
  const res = await api<TrackedListResponse>(
    `/api/repos/tracked${q ? `?${q}` : ""}`,
  );
  return res.items ?? [];
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
