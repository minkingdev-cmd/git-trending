export class UnauthorizedError extends Error {
  constructor() {
    super("unauthorized");
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

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
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
    if (!res.ok) {
      throw new UnauthorizedError();
    }
    return res.json();
  }
  if (!res.ok) {
    if (res.status === 401) {
      throw new UnauthorizedError();
    }
    throw new Error(`HTTP ${res.status}`);
  }
  return res.json();
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
