import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { api, UnauthorizedError, compact } from "./api";

describe("compact", () => {
  it("formats numbers compactly", () => {
    expect(compact(190000)).toBe("190K");
    expect(compact(950)).toBe("950");
  });
});

describe("api 401 handling", () => {
  beforeEach(() => vi.restoreAllMocks());
  afterEach(() => vi.restoreAllMocks());

  it("retries once after successful refresh", async () => {
    const fetchMock = vi
      .fn()
      // first business request 401
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      // refresh succeeds
      .mockResolvedValueOnce(new Response(null, { status: 200 }))
      // replay succeeds
      .mockResolvedValueOnce(new Response(JSON.stringify({ ok: 1 }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const data = await api<{ ok: number }>("/api/leaderboard/trending");
    expect(data.ok).toBe(1);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(fetchMock.mock.calls[1][0]).toBe("/api/auth/refresh");
  });

  it("throws UnauthorizedError when refresh fails", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(new Response(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/leaderboard/trending")).rejects.toBeInstanceOf(UnauthorizedError);
  });

  it("does not retry 401 on auth endpoints", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/auth/me")).rejects.toBeInstanceOf(UnauthorizedError);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
